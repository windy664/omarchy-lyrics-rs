# 03 — 已知 bug 与线索

## Bug A：顶栏小部件永不渲染（本项目的立项原因）

- 现象：插件装上后 service 看似未加载，**无任何报错日志**，顶栏无小部件。
- 环境限定：Omarchy 4.0.6 / Quickshell 0.3.2（用户真机复现）。
- Issue：[crmne/omarchy-lyrics#22](https://github.com/crmne/omarchy-lyrics/issues/22)，状态 OPEN（截至 2026-10-31），作者零回复。
- 我们的排查评论在 issue 里（用户留的）。

## Bug B：hideWhenIdle 字符串/布尔混淆（Copilot 机器人 2026-10-29 诊断）

机器人在 issue 下的诊断原文要点：

> The saved `hideWhenIdle` value is a separate issue: the widget only treats the boolean `true` as enabled, so a stored string `"false"` does not disable hiding. With no service media state, that leaves the widget at **zero width**.
> 出处指向 [BarWidget.qml @ 91a3ba2](https://github.com/crmne/omarchy-lyrics/blob/91a3ba239705fab559a6b3f551636474211d5798/BarWidget.qml)

翻译：设置存储层如果存的是字符串 `"false"`，代码里"只在值为布尔 `true` 时启用"的判断会让它走隐藏分支；叠加"没有媒体状态"这个条件，小部件就被压成**零宽度**——看起来就像"没渲染"。

**解读**：两个 bug 可能是同一条因果链——A 的表象（看似未渲染）至少部分由 B 的零宽度机制造成。但"service appears unloaded"的观察仍无法完全用 B 解释，所以 **A 是否纯粹是 B 的表象尚未证实**。

## ⚠️ 未验证假设清单（接手后按顺序验证）

1. **验证 B 的因果链**（最高优先，成本 5 分钟）：
   - 装 v1.2.8：`omarchy plugin add https://github.com/crmne/omarchy-lyrics --enable`
   - 找到它持久化的设置值，检查 `hideWhenIdle` 存的是字符串还是布尔（先 `omarchy plugin list` 确认 ID，再翻 `~/.local/state/omarchy/` 和 `~/.config/omarchy/` 下的插件设置文件）
   - 若是字符串：手动改成布尔后重启 quickshell（或重载插件），看小部件是否出现
2. **验证 omarchy.media 依赖**：本机 `omarchy.media` 是 disabled（见 docs/01）。启用它（`omarchy plugin enable omarchy.media`）再测——若歌词 service 因此加载，说明原插件隐式依赖 first-party media service。这个事实对重写架构有直接影响（我们计划直连 MPRIS，绕开这个依赖）。
3. **Spotify 的 MPRIS 可用性**：`busctl --user list | grep -i mpris` 或 `playerctl -l`（playerctl 未装则 pacman 装）。确认放歌时 MPRIS 元数据齐全（artist/title/duration/artUrl）。

## 验证结果登记（接手 AI 填写）

| # | 假设 | 结果 | 日期 |
|---|------|------|------|
| 1 | hideWhenIdle 字符串导致零宽度 | ☐ 待验证 | |
| 2 | 隐式依赖 omarchy.media | ☐ 待验证 | |
| 3 | Spotify MPRIS 元数据完整 | ☐ 待验证 | |

## 经验教训（写进重写设计）

1. **读设置必须带类型归一化**：任何从存储层读出的值，先 coerce（`"false"`/`"true"`/`""` → 布尔），Rust 侧用强类型解析天然免疫，QML 侧也要显式 `=== true` 判断并打日志。
2. **静默失败是第一大敌**：原插件"无报错但不渲染"拖了一个多星期。重写版要求：service 启动、MPRIS 连接、LRCLIB 请求三个关键节点必须留可查日志（quickshell 日志或本地状态文件）。
3. **外部依赖要有降级**：LRCLIB 不可达时显示 plain lyrics 或"暂无歌词"，不要让网络故障导致部件消失。

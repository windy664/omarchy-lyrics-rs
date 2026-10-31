# omarchy-lyrics-rs — Omarchy 顶栏内嵌歌词插件（Rust 重写）

## 一句话目标

做一个功能对标 [crmne/omarchy-lyrics](https://github.com/crmne/omarchy-lyrics) 的 Omarchy 顶栏歌词插件，但**后端用 Rust 重写**：从 MPRIS 拿当前播放 → 从 LRCLIB 拉带时间戳的歌词 → 在顶栏按行滚动高亮。

## 为什么存在（三句话）

1. 原插件在 Omarchy 4.0.6 / Quickshell 0.3.2 上顶栏永不渲染，报了 [issue #22](https://github.com/crmne/omarchy-lyrics/issues/22)，作者长期无回应，只剩机器人在 2026-10-29 留了一条诊断（见 `docs/03-已知bug与线索.md`）。
2. 原插件是 Python + QML/JS，用户明确偏好 Rust，且那条机器人诊断的 bug（字符串 `"false"` 不被当布尔值）正是动态类型的经典翻车。
3. 用户决定不再等作者，自己重写，交给 AI 完成。

## 文档导航（按顺序读）

| 文件 | 内容 |
|------|------|
| [docs/00-背景.md](docs/00-背景.md) | 事情的来龙去脉、决策记录 |
| [docs/01-环境.md](docs/01-环境.md) | 本机 Omarchy 环境硬事实、插件机制、调试方法 |
| [docs/02-原插件架构分析.md](docs/02-原插件架构分析.md) | crmne/omarchy-lyrics 逐文件拆解 |
| [docs/03-已知bug与线索.md](docs/03-已知bug与线索.md) | issue #22 详情、hideWhenIdle 隐身 bug、未验证的假设 |
| [docs/04-重写设计方案.md](docs/04-重写设计方案.md) | Rust 后端 + QML 前端的目标架构 |
| [docs/05-任务清单与验收.md](docs/05-任务清单与验收.md) | 分阶段任务、每步验收标准 |
| [docs/06-协作规范.md](docs/06-协作规范.md) | 用户偏好、红线、沟通方式（**接手必读**） |

## 30 秒上手路径

```bash
# 1. 读 docs/06-协作规范.md（怎么和用户打交道）
# 2. 读 docs/03-已知bug与线索.md（知道坑在哪）
# 3. 读 docs/04-重写设计方案.md（要建什么）
# 4. 按 docs/05-任务清单与验收.md 开工
```

## 当前状态（2026-10-31）

- [x] 背景调研、原插件逆向、bug 线索收集 —— 已完成（见 docs）
- [x] 交接文档 —— 本仓库
- [ ] 验证 Copilot 机器人对 hideWhenIdle 的诊断（最快的第一步，见 docs/03）
- [ ] Rust 后端骨架
- [ ] QML 小部件
- [ ] 真机验收（用户在 Spotify 放歌，顶栏出同步歌词）

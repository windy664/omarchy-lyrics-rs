# omarchy-lyrics-rs

Omarchy 顶栏歌词插件（Rust 重写版）：正在播放时，顶栏右侧显示 **封面缩略图 + 当前歌词条**，词级歌词逐字点亮卡拉OK，点击展开歌词全文。

![效果图](docs/mockup-v1.png)

## 特性

- **顶栏歌词**：封面（20px 圆角）+ 当前行，扁平融入 Omarchy 顶栏，换行带宽度动画
- **词级卡拉OK**：已唱字加粗亮色、未唱字置灰，逐字推进（源：lrc.red / karalyr，词轴并发竞争）
- **多源降级**：词级 → LRCLIB 行级同步 → LRCLIB 纯文本 → “暂无歌词”（部件不消失）
- **歌词面板**：点击小部件展开全文；默认自动跟随当前行，手动滚动进入浏览模式（2.5s 自动恢复 + “回到当前行”）；点击任意行跳播
- **主题跟随**：颜色全部取自 Omarchy 主题，昼夜切换无需重载
- **稳定设计**：DBus/HTTP/时间同步全在 Rust 后端 lyricsd，QML 只渲染；后端崩溃指数退避自动重启

## 安装

依赖：Omarchy（Quickshell 0.3+）、Rust 工具链（构建后端）。

```bash
# 1. 构建并安装后端
git clone https://github.com/windy664/omarchy-lyrics-rs.git
cd omarchy-lyrics-rs/backend
cargo build --release
cp target/release/lyricsd ~/.local/bin/

# 2. 安装插件
omarchy plugin add https://github.com/windy664/omarchy-lyrics-rs --enable
```

在 Omarchy 设置里把 **Lyrics** 小部件加到顶栏即可。

## 设置

| 键 | 说明 | 默认 |
|----|------|------|
| `hideWhenIdle` | 无播放（暂停）时隐藏小部件 | `true` |
| `panelWidthPercent` | 歌词面板宽度（屏幕百分比） | `30` |
| `panelHeightPercent` | 歌词面板高度（屏幕百分比） | `85` |

## 歌词源

| 优先级 | 源 | 说明 |
|--------|-----|------|
| 1 | lrc.red | Apple Music TTML 词级时间轴 |
| 1 | karalyr.com | 开放卡拉OK库，词级（Enhanced LRC） |
| 2 | lrclib.net | 行级同步歌词（带 5xx 退避重试） |
| 3 | lrclib.net | 纯文本按时长均分 |

查询结果按曲目+时长（15 秒桶）落盘缓存于 `~/.local/state/omarchy-lyrics-rs/`。

## 与原插件的关系

功能对标 [crmne/omarchy-lyrics](https://github.com/crmne/omarchy-lyrics)，因其在 Omarchy 4.0.6 上顶栏不渲染（[issue #22](https://github.com/crmne/omarchy-lyrics/issues/22)）且无回应而重写。架构上把会出错的逻辑（DBus、HTTP、歌词定位、类型解析）全部移入 Rust，QML 只做渲染——原插件的隐身 bug（设置存字符串 `"false"` 不被当布尔）在这套架构下天然免疫。开发过程分析见 [docs/](docs/00-背景.md)。

## License

[MIT](LICENSE)

import QtQuick
import Quickshell
import Quickshell.Io

// 薄壳：拉起/监控 lyricsd（Rust 后端），把 stdio 行式 JSON 事件转成属性，
// BarWidget 经 bar.shell.serviceFor("local.lyrics-rs") 直接读这些属性。
// 所有业务逻辑（DBus/HTTP/时间同步）都在 Rust，这里禁止业务逻辑。
Item {
  id: root

  // Omarchy Shell 宿主注入
  property var shell: null
  property var manifest: null

  // ── 对外共享状态（BarWidget/LyricsView 只读） ──────────────────────────
  property var track: ({})          // {artist,title,album,durationSec,artUrl}
  property var lines: []            // [{t,text,words:[[ms,word],...]|null}]
  property string linesSource: ""   // word:lrc.red | word:karalyr | lrclib | lrclib-plain
  property int lineIndex: -1        // 当前行下标
  property string lineRich: ""      // 当前行 StyledText（已唱词加粗/未唱置灰）
  property string lineText: ""      // 当前行纯文本
  property int wordIndex: -1        // 行内当前词下标（词级源才有）
  property bool playing: false
  property real positionSec: 0
  property string status: ""        // no_lyrics / ad 等
  property string statusMsg: ""     // status 附带文案（广告文案等）
  property bool backendUp: false
  property bool panelRequested: false // IPC 远程开面板（CLI 调试/自动化用）

  readonly property bool hasTrack: track && track.title ? true : false

  function sendCommand(cmd) {
    if (!proc.running || !proc.stdinEnabled) {
      console.warn("[lyrics] sendCommand failed: backend not running")
      return
    }
    try {
      proc.write(JSON.stringify(cmd) + "\n")
    } catch (e) {
      console.warn("[lyrics] sendCommand error:", e)
    }
  }

  function refresh() {
    sendCommand({ "cmd": "refresh" })
  }

  // ── 后端生命周期：拉起 + 指数退避重启 ────────────────────────────────────
  property int exitCount: 0
  property real startedAt: 0
  readonly property int restartDelayMs: Math.min(30000, 1000 * Math.pow(2, Math.min(exitCount, 5)))

  function launch() {
    const bin = Quickshell.env("HOME") + "/.local/bin/lyricsd"
    proc.command = [bin]
    startedAt = Date.now()
    proc.running = true
  }

  Process {
    id: proc
    property bool stdinEnabled: true

    stdout: SplitParser {
      onRead: function(data) {
        try {
          root.handleEvent(JSON.parse(data))
        } catch (e) {
          console.warn("[lyrics] bad json line:", data, e)
        }
      }
    }
    stderr: SplitParser {
      onRead: function(data) {
        console.log("[lyricsd]", data)
      }
    }
    onRunningChanged: function(running) {
      if (running) {
        root.backendUp = true
        console.log("[lyrics] lyricsd started, pid", proc.pid)
      }
    }
    onExited: function(code, status) {
      root.backendUp = false
      const uptime = Date.now() - root.startedAt
      if (uptime > 60000) root.exitCount = 0
      else root.exitCount += 1
      console.warn("[lyrics] lyricsd exited code=" + code + " uptimeMs=" + Math.round(uptime)
                   + " restart in " + root.restartDelayMs + "ms")
      restartTimer.restart()
    }
  }

  Timer {
    id: restartTimer
    interval: root.restartDelayMs
    onTriggered: root.launch()
  }

  Component.onCompleted: {
    console.log("[lyrics] service starting backend")
    launch()
  }

  // ── 事件分发（后端 → 属性） ──────────────────────────────────────────────
  function handleEvent(ev) {
    switch (ev.ev) {
      case "backend":
        console.log("[lyrics] backend:", ev.msg)
        break
      case "track":
        root.track = ev.title ? ev : {}
        break
      case "lines":
        root.lines = ev.synced || []
        root.linesSource = ev.source || ""
        break
      case "line":
        root.lineIndex = ev.index !== undefined ? ev.index : -1
        root.lineRich = ev.rich || ""
        root.lineText = ev.text || ""
        root.wordIndex = ev.word !== undefined ? ev.word : -1
        break
      case "position":
        root.playing = ev.playing === true
        root.positionSec = ev.sec || 0
        break
      case "status":
        root.status = ev.msg || ""
        root.statusMsg = ev.text || ""
        break
      default:
        break
    }
  }

  IpcHandler {
    target: "local.lyrics-rs"

    function refresh(): void {
      root.refresh()
    }
    function panel(): void {
      root.panelRequested = true
    }
    function status(): string {
      return JSON.stringify({
        track: root.track.title || "",
        playing: root.playing,
        line: root.lineIndex,
        source: root.linesSource,
        backend: root.backendUp
      })
    }
  }
}

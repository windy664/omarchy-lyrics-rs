import QtQuick
import QtQuick.Effects
import Quickshell
import qs.Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "local.lyrics-rs"

  // 共享状态：Service 实例由宿主注册，这里只读
  readonly property var lyricsService: bar && bar.shell ? bar.shell.serviceFor("local.lyrics-rs") : null
  readonly property bool hasTrack: lyricsService && lyricsService.hasTrack ? true : false
  readonly property bool isPlaying: lyricsService ? lyricsService.playing : false

  // docs/03 教训 #1：设置读回类型不可信，必须归一化
  readonly property bool hideWhenIdleNormalized: {
    const v = setting("hideWhenIdle", true)
    return v === true || v === "true"
  }

  Component.onCompleted: {
    console.log("[lyrics] hideWhenIdle raw =", JSON.stringify(setting("hideWhenIdle", true)),
                "→ normalized =", hideWhenIdleNormalized)
  }

  // rich 里后端输出的未唱词置灰是固定 #888888，替换为主题 muted 色，避免昼夜切换翻车
  readonly property string themedRich: lyricsService && lyricsService.lineRich !== ""
    ? String(lyricsService.lineRich).replace(/#888888/g, String(Color.muted)) : ""

  readonly property bool compactVisible: hasTrack && (!hideWhenIdleNormalized || isPlaying)

  // 广告插播中（后端识别 Spotify 广告位：artist 空 → status=ad + 广告文案）
  // 注意：必须挂在 root 上——此前定义在 Text 内部，绑定里 root.isAd 是 undefined，
  // QML 静默返回 falsy，广告字幕从未生效过
  readonly property bool isAd: lyricsService ? lyricsService.status === "ad" : false

  // 手势设置：滚轮动作（track=切歌 / volume=调音量 / none），经 manifest schema 暴露
  readonly property string wheelAction: {
    const v = setting("wheelAction", "track")
    return (v === "volume" || v === "none") ? v : "track"
  }

  property real wheelAccum: 0

  function sendPlayer(cmd) {
    if (lyricsService) lyricsService.sendCommand(cmd)
  }

  // IPC 远程开面板（omarchy shell local.lyrics-rs panel）
  Connections {
    target: root.lyricsService
    function onPanelRequestedChanged() {
      if (root.lyricsService && root.lyricsService.panelRequested) {
        panel.open = true
        root.lyricsService.panelRequested = false
      }
    }
  }

  implicitHeight: barSize
  implicitWidth: contentRow.visible ? contentRow.implicitWidth : 0
  Behavior on implicitWidth {
    NumberAnimation { duration: 200; easing.type: Easing.OutCubic }
  }

  Row {
    id: contentRow
    visible: compactVisible
    anchors.verticalCenter: parent.verticalCenter
    spacing: Style.space(8)

    // 封面缩略图（约 20px 圆角，MultiEffect 遮罩实现真圆角）
    Item {
      width: Style.space(20)
      height: Style.space(20)
      anchors.verticalCenter: parent.verticalCenter
      visible: root.hasTrack && root.lyricsService.track.artUrl !== ""

      Rectangle {
        id: coverMask
        anchors.fill: parent
        radius: Style.space(6)
        visible: false
        layer.enabled: true
      }

      Image {
        anchors.fill: parent
        visible: root.hasTrack && root.lyricsService.track.artUrl !== ""
        source: root.hasTrack ? root.lyricsService.track.artUrl : ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        cache: true
        layer.enabled: true
        layer.effect: MultiEffect {
          maskEnabled: true
          maskSource: coverMask
          maskThresholdMin: 0.5
          maskSpreadAtMin: 1.0
        }
      }
    }

    // 当前行歌词条（StyledText，词级已唱加粗/未唱置灰）；广告插播时显示广告文案
    Text {
      anchors.verticalCenter: parent.verticalCenter
      text: root.isAd ? (root.lyricsService.statusMsg || "广告插播中")
            : root.themedRich !== ""
            ? root.themedRich
            : (root.hasTrack ? root.lyricsService.lineText : "")
      textFormat: Text.StyledText
      font.family: root.bar ? root.bar.fontFamily : Style.font.family
      font.pixelSize: Style.font.body
      color: root.bar ? root.bar.barForeground : Color.bar.text
      elide: Text.ElideRight
      verticalAlignment: Text.AlignVCenter
    }
  }

  // 手势基座：第一方 WidgetButton（clock/microphone 同款），支持左/右/中键 + 滚轮
  // 左键=开关面板；右键=播放暂停；中键=下一曲；滚轮=切歌（或按设置调音量）
  WidgetButton {
    anchors.fill: parent
    bar: root.bar
    text: ""
    keepSpace: true // text 为空时 hasVisualContent=false 会整层隐藏，必须 keepSpace 才收得到手势
    interactive: root.compactVisible
    onPressed: function(b) {
      if (!root.compactVisible) return
      if (b === Qt.RightButton) root.sendPlayer({ "cmd": "toggle" })
      else if (b === Qt.MiddleButton) root.sendPlayer({ "cmd": "next" })
      else panel.open = !panel.open
    }
    onWheelMoved: function(delta) {
      if (!root.compactVisible) return
      if (root.wheelAction === "none") return
      if (root.wheelAction === "volume") {
        const r = Util.wheelSteps(root.wheelAccum, delta)
        root.wheelAccum = r.remainder
        for (let i = 0; i < Math.abs(r.steps); i++)
          Util.execDetached("wpctl set-volume @DEFAULT_AUDIO_SINK@ " + (r.steps > 0 ? "5%+" : "5%-"))
      } else {
        // 与第一方 media 组件同向：滚轮向上=上一曲
        root.sendPlayer(delta > 0 ? { "cmd": "prev" } : { "cmd": "next" })
      }
    }
  }

  // 展开歌词面板（点击展开，不是 hover；尺寸按 shell.json 设置的屏幕百分比）
  KeyboardPanel {
    id: panel
    anchorItem: root
    owner: root
    bar: root.bar
    centerOnBar: false

    readonly property var scr: (bar && bar.screen) ? bar.screen : Quickshell.screens[0]
    contentWidth: Math.round((scr ? scr.width : 1200) * root.setting("panelWidthPercent", 30) / 100)
    // 高度贴合内容（3 行歌词 + 控制条），不再吃满屏幕百分比
    contentHeight: contentCol.implicitHeight + verticalContentInset

    Column {
      id: contentCol
      anchors.fill: parent
      spacing: Style.space(10)

      LyricsView {
        width: parent.width
        height: implicitHeight
        service: root.lyricsService
      }

      ControlsBar {
        id: controls
        width: parent.width
        visible: root.hasTrack
        service: root.lyricsService
        bar: root.bar
      }
    }
  }
}

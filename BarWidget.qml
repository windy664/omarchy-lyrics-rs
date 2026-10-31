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

    // 当前行歌词条（StyledText，词级已唱加粗/未唱置灰）
    Text {
      anchors.verticalCenter: parent.verticalCenter
      text: root.themedRich !== "" ? root.themedRich
            : (root.hasTrack ? root.lyricsService.lineText : "")
      textFormat: Text.StyledText
      font.family: root.bar ? root.bar.fontFamily : Style.font.family
      font.pixelSize: Style.font.body
      color: root.bar ? root.bar.barForeground : Color.bar.text
      elide: Text.ElideRight
      verticalAlignment: Text.AlignVCenter
    }
  }

  MouseArea {
    anchors.fill: parent
    enabled: compactVisible
    cursorShape: compactVisible ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: panel.open = !panel.open
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
    contentHeight: Math.round((scr ? scr.height : 800) * root.setting("panelHeightPercent", 85) / 100)

    LyricsView {
      service: root.lyricsService
      anchors.fill: parent
    }
  }
}

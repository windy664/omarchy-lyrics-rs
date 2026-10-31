import QtQuick
import qs.Commons
import qs.Ui

// 歌词全文面板：ListView 渲染，QML 只做渲染和点击，不做时间计算。
// 默认自动跟随当前行；用户滚动进入浏览模式，2.5s 后自动回到跟随；
// 点击行跳播（后端经 MPRIS SetPosition，不支持时后端自行忽略）。
Item {
  id: root

  property var service: null
  readonly property int currentLine: service ? service.lineIndex : -1
  readonly property bool hasLyrics: service && service.lines && service.lines.length > 0

  // 浏览模式：手动滚动后暂停自动跟随
  property bool browseMode: false

  function follow() {
    if (browseMode || !hasLyrics || currentLine < 0) return
    if (listView.count <= 0) return
    listView.positionViewAtIndex(Math.min(currentLine, listView.count - 1), ListView.Contain)
  }

  function enterBrowse() {
    browseMode = true
    returnTimer.restart()
  }

  Timer {
    id: returnTimer
    interval: 2500
    onTriggered: {
      root.browseMode = false
      root.follow()
    }
  }

  onCurrentLineChanged: follow()
  onHasLyricsChanged: {
    if (hasLyrics) follow()
  }

  // 暂无歌词兜底文案（部件不消失，由 BarWidget 保证）
  Text {
    anchors.centerIn: parent
    visible: !hasLyrics
    text: service && service.hasTrack
          ? (service.status === "no_lyrics" ? "暂无歌词" : "正在查找歌词…")
          : "没有正在播放的音乐"
    font.family: Style.font.family
    font.pixelSize: Style.font.subtitle
    color: Color.muted
  }

  ListView {
    id: listView
    anchors.fill: parent
    anchors.margins: Style.space(16)
    visible: root.hasLyrics
    clip: true
    spacing: Style.space(6)
    model: root.service ? root.service.lines : []

    onMovementStarted: root.enterBrowse()
    onCountChanged: root.follow()

    delegate: Item {
      required property var modelData
      required property int index

      width: listView.width
      height: lineText.implicitHeight + Style.space(4)

      readonly property int distance: Math.abs(index - root.currentLine)
      readonly property bool isCurrent: index === root.currentLine

      opacity: isCurrent ? 1.0 : (distance <= 3 ? 0.55 : 0.3)
      Behavior on opacity { NumberAnimation { duration: 180 } }

      Text {
        id: lineText
        width: parent.width
        // 当前行用后端推送的 rich（词级逐词点亮），其余行纯文本
        text: isCurrent && root.service && root.service.lineRich !== ""
              ? String(root.service.lineRich).replace(/#888888/g, String(Color.muted))
              : modelData.text
        textFormat: Text.StyledText
        font.family: Style.font.family
        font.pixelSize: root.isCurrent ? Style.font.heading : Style.font.subtitle
        font.bold: isCurrent
        color: isCurrent ? Color.accent : Color.muted
        wrapMode: Text.WordWrap
        verticalAlignment: Text.AlignVCenter
      }

      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: {
          if (root.service && modelData && modelData.t !== undefined) {
            root.service.sendCommand({ "cmd": "seek", "sec": modelData.t })
          }
        }
      }
    }
  }

  // "回到当前行"按钮（仅浏览模式显示）
  Rectangle {
    anchors.bottom: parent.bottom
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottomMargin: Style.space(12)
    width: backText.implicitWidth + Style.space(20)
    height: backText.implicitHeight + Style.space(8)
    radius: Style.space(10)
    visible: browseMode && hasLyrics
    color: Color.composed("bar.background", "bar.background-alpha", Color.background, 0.9)
    border.width: 1
    border.color: Color.popups.border

    Text {
      id: backText
      anchors.centerIn: parent
      text: "回到当前行"
      font.family: Style.font.family
      font.pixelSize: Style.font.bodySmall
      color: Color.bar.text
    }

    MouseArea {
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: {
        returnTimer.stop()
        root.browseMode = false
        root.follow()
      }
    }
  }
}

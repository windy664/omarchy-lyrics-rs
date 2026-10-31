import QtQuick
import qs.Commons
import qs.Ui

// 歌词全文面板：固定 5 行视窗，当前行锁定正中间（上 2 行 + 当前行 + 下 2 行）。
// 不滚动、不浏览；点击某行跳播（后端经 MPRIS SetPosition，不支持时后端自行忽略）。
Item {
  id: root

  property var service: null
  readonly property int currentLine: service ? service.lineIndex : -1

  readonly property bool hasLyrics: service && service.lines && service.lines.length > 0
  // 单行槽位高度：容纳当前行的大号字体，5 槽 = 视窗总高
  readonly property real rowH: Style.space(38)
  readonly property int windowRows: 3

  // 面板高度自适应内容：有词时 = 5 行视窗，无词时 = 兜底文案高度
  implicitHeight: hasLyrics ? rowH * windowRows : Style.space(80)

  // 暂无歌词兜底文案（部件不消失，由 BarWidget 保证）
  Text {
    anchors.centerIn: parent
    visible: !root.hasLyrics
    text: service && service.hasTrack
          ? (service.status === "ad" ? (service.statusMsg || "广告插播中")
            : service.status === "no_lyrics" ? "暂无歌词" : "正在查找歌词…")
          : "没有正在播放的音乐"
    font.family: Style.font.family
    font.pixelSize: Style.font.subtitle
    color: Color.muted
  }

  ListView {
    id: listView
    visible: root.hasLyrics
    clip: true
    width: parent.width
    height: root.rowH * root.windowRows
    anchors.verticalCenter: parent.verticalCenter
    anchors.leftMargin: Style.space(16)
    anchors.rightMargin: Style.space(16)

    model: root.service ? root.service.lines : []
    currentIndex: root.currentLine
    interactive: false

    // StrictlyEnforceRange：无论怎么动，当前行永远钉在视窗正中的槽位
    highlightRangeMode: ListView.StrictlyEnforceRange
    preferredHighlightBegin: height / 2 - root.rowH / 2
    preferredHighlightEnd: height / 2 + root.rowH / 2
    snapMode: ListView.SnapToItem
    cacheBuffer: root.rowH * 4

    Behavior on height { NumberAnimation { duration: 150 } }

    delegate: Item {
      required property var modelData
      required property int index

      width: listView.width
      height: root.rowH

      readonly property bool isCurrent: index === root.currentLine
      // iOS Apple Music 同款：全程只有主题主文字色一种色调，
      // 当前行实、未唱词虚（约 40% 透明度），非当前行纯透明度淡化，不掺 accent 等第三色
      readonly property string dimForeground: {
        const f = String(Color.foreground)
        return "#" + "66" + (f[0] === "#" ? f.slice(1) : f)
      }

      opacity: isCurrent ? 1.0 : (Math.abs(index - root.currentLine) === 1 ? 0.62 : 0.45)
      Behavior on opacity { NumberAnimation { duration: 180 } }

      Text {
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width
        // 当前行用后端推送的 rich（词级逐词点亮），其余行纯文本
        text: isCurrent && root.service && root.service.lineRich !== ""
              ? String(root.service.lineRich).replace(/#888888/g, root.dimForeground)
              : modelData.text
        textFormat: Text.StyledText
        font.family: Style.font.family
        font.pixelSize: root.isCurrent ? Style.font.heading : Style.font.subtitle
        font.bold: isCurrent
        color: Color.foreground
        // 单行槽位：超长行省略号截断，保证视窗永远是整齐的 3 行；整行水平居中
        elide: Text.ElideMiddle
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter

        Behavior on color { ColorAnimation { duration: 250 } }
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
}

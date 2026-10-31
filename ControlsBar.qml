import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

// 面板底部控制区：随机/循环/上一曲/播放暂停/下一曲/重匹配歌词 + seek 进度条。
// 全部经 Service.sendCommand 下发；MPRIS 不可用时后端自行忽略。
// 注意：Spotify 官方 Linux 客户端不接受 MPRIS 对 Shuffle/LoopStatus 的写入（只读），
// 按钮在该播放器上显示真实状态但点击无效；mpv/VLC 等正常播放器完全可用。
Item {
  id: root

  property var service: null
  property var bar: null

  readonly property real duration: service && service.track && service.track.durationSec > 0 ? service.track.durationSec : 0
  readonly property bool playing: service ? service.playing : false
  readonly property bool shuffleOn: service ? service.shuffle === true : false
  readonly property string loopStatus: service ? (service.loopStatus || "None") : "None"

  implicitHeight: col.implicitHeight

  function send(cmd) {
    if (service) service.sendCommand(cmd)
  }

  function fmt(sec) {
    var s = Math.max(0, Math.floor(sec))
    return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
  }

  component CtrlButton: Text {
    property var action: null
    property bool dimmed: false

    font.family: Style.font.family
    font.pixelSize: Style.font.subtitle
    color: "#FFFFFF"
    // iOS 风：只用前景色明暗表达状态，不掺 accent
    opacity: dimmed ? 0.35 : (ma.containsMouse ? 1.0 : 0.8)
    Behavior on opacity { NumberAnimation { duration: 150 } }

    MouseArea {
      id: ma
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: if (parent.action) parent.action()
    }
  }

  // 主题色滑杆（自绘替代宿主 PanelSlider，去 accent 蓝）：槽/已播/手柄全是前景色明暗
  component ThemeSlider: Item {
    id: slider
    property real maximum: 1
    property real value: 0
    property bool dragging: false
    property real dragValue: 0
    readonly property real shown: dragging ? dragValue : value
    readonly property real ratio: maximum > 0 ? Math.min(Math.max(shown / maximum, 0), 1) : 0
    signal released(real v)

    implicitHeight: Style.space(16)

    Rectangle {
      anchors.verticalCenter: parent.verticalCenter
      width: parent.width
      height: 3
      radius: 1.5
      color: "#FFFFFF"
      opacity: 0.16
    }
    Rectangle {
      anchors.verticalCenter: parent.verticalCenter
      width: parent.width * slider.ratio
      height: 3
      radius: 1.5
      color: "#FFFFFF"
      opacity: 0.9
    }
    Rectangle {
      anchors.verticalCenter: parent.verticalCenter
      x: Math.min(Math.max(slider.width * slider.ratio - width / 2, 0), Math.max(slider.width - width, 0))
      width: slider.dragging || ma.containsMouse ? 12 : 8
      height: width
      radius: width / 2
      color: "#FFFFFF"
      Behavior on width { NumberAnimation { duration: 100 } }
    }

    MouseArea {
      id: ma
      anchors.fill: parent
      anchors.margins: -Style.space(6)
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      function pick(x) {
        slider.dragValue = Math.min(Math.max(x / slider.width, 0), 1) * slider.maximum
      }
      onPressed: function(mouse) {
        slider.dragging = true
        pick(mouse.x)
      }
      onPositionChanged: function(mouse) {
        if (slider.dragging) pick(mouse.x)
      }
      onReleased: function(mouse) {
        pick(mouse.x)
        slider.dragging = false
        slider.released(slider.dragValue)
      }
      onCanceled: slider.dragging = false
    }
  }

  ColumnLayout {
    id: col
    anchors.left: parent.left
    anchors.right: parent.right
    spacing: Style.space(10)

    RowLayout {
      Layout.fillWidth: true
      spacing: Style.space(20)

      // 左：随机 / 循环
      CtrlButton {
        text: "󰒓"
        dimmed: !root.shuffleOn
        action: function() { root.send({ "cmd": "shuffle" }) }
      }
      CtrlButton {
        text: root.loopStatus === "Track" ? "󰑙" : "󰑗"
        dimmed: root.loopStatus === "None"
        action: function() { root.send({ "cmd": "loop" }) }
      }

      Item { Layout.fillWidth: true }

      // 中：上一曲 / 播放暂停 / 下一曲
      CtrlButton {
        text: "󰒮"
        action: function() { root.send({ "cmd": "prev" }) }
      }
      CtrlButton {
        font.pixelSize: Style.font.heading
        text: root.playing ? "󰏤" : "󰐊"
        action: function() { root.send({ "cmd": "toggle" }) }
      }
      CtrlButton {
        text: "󰒭"
        action: function() { root.send({ "cmd": "next" }) }
      }

      Item { Layout.fillWidth: true }

      // 右：重匹配歌词（bypass 缓存重查词源）
      CtrlButton {
        text: "󰑐"
        action: function() { root.send({ "cmd": "refresh" }) }
      }
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: Style.space(10)

      Text {
        text: root.fmt(root.service ? root.service.positionSec : 0)
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        color: "#9A9A9A"
      }

      ThemeSlider {
        implicitWidth: Style.space(160)
        Layout.fillWidth: true
        enabled: root.duration > 0
        maximum: root.duration > 0 ? root.duration : 1
        value: root.duration > 0 && root.service ? Math.min(root.service.positionSec, root.duration) : 0
        onReleased: function(v) {
          if (root.duration > 0) root.send({ "cmd": "seek", "sec": v })
        }
      }

      Text {
        text: root.fmt(root.duration)
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        color: "#9A9A9A"
      }
    }
  }
}

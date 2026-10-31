import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

// 面板底部播放控制条（DMP 基准）：上一曲 / 播放暂停 / 下一曲 + seek 进度条。
// 全部经 Service.sendCommand 下发，MPRIS 不可用时后端自行忽略。
Item {
  id: root

  property var service: null
  property var bar: null

  readonly property real duration: service && service.track && service.track.durationSec > 0 ? service.track.durationSec : 0
  readonly property bool playing: service ? service.playing : false

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

    font.family: Style.font.family
    font.pixelSize: Style.font.subtitle
    color: ma.containsMouse ? Color.accent : Color.bar.text

    MouseArea {
      id: ma
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: if (parent.action) parent.action()
    }
  }

  ColumnLayout {
    id: col
    anchors.left: parent.left
    anchors.right: parent.right
    spacing: Style.space(10)

    RowLayout {
      Layout.fillWidth: true
      spacing: Style.space(24)

      Item { Layout.fillWidth: true }

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
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: Style.space(10)

      Text {
        text: root.fmt(root.service ? root.service.positionSec : 0)
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        color: Color.muted
      }

      PanelSlider {
        Layout.fillWidth: true
        bar: root.bar
        enabled: root.duration > 0
        maximum: root.duration > 0 ? root.duration : 1
        value: !dragging && root.duration > 0 && root.service ? Math.min(root.service.positionSec, root.duration) : 0
        onReleased: function(v) {
          if (root.duration > 0) root.send({ "cmd": "seek", "sec": v })
        }
      }

      Text {
        text: root.fmt(root.duration)
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        color: Color.muted
      }
    }
  }
}

"""Draw the app icon (gradient tile + terminal prompt + spark) to web/icon.png and sshpro.ico."""

from pathlib import Path

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QBrush, QColor, QGuiApplication, QImage, QLinearGradient, QPainter, QPainterPath, QPen

ROOT = Path(__file__).resolve().parents[1]


def draw(size: int) -> QImage:
    img = QImage(size, size, QImage.Format.Format_ARGB32)
    img.fill(Qt.GlobalColor.transparent)
    p = QPainter(img)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    s = size / 256
    grad = QLinearGradient(0, 0, size, size)
    grad.setColorAt(0, QColor("#22d3ee"))
    grad.setColorAt(0.55, QColor("#6366f1"))
    grad.setColorAt(1, QColor("#a855f7"))
    tile = QPainterPath()
    tile.addRoundedRect(QRectF(8 * s, 8 * s, 240 * s, 240 * s), 58 * s, 58 * s)
    p.fillPath(tile, QBrush(grad))
    pen = QPen(QColor("white"), 22 * s, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap, Qt.PenJoinStyle.RoundJoin)
    p.setPen(pen)
    p.drawPolyline([QPointF(62 * s, 82 * s), QPointF(112 * s, 130 * s), QPointF(62 * s, 178 * s)])
    p.drawLine(QPointF(132 * s, 182 * s), QPointF(190 * s, 182 * s))
    spark = QPainterPath()
    cx, cy, r = 182 * s, 76 * s, 30 * s
    spark.moveTo(cx, cy - r)
    spark.quadTo(cx, cy, cx + r, cy)
    spark.quadTo(cx, cy, cx, cy + r)
    spark.quadTo(cx, cy, cx - r, cy)
    spark.quadTo(cx, cy, cx, cy - r)
    p.setPen(Qt.PenStyle.NoPen)
    p.fillPath(spark, QColor("white"))
    p.end()
    return img


if __name__ == "__main__":
    app = QGuiApplication([])
    draw(256).save(str(ROOT / "sshpro" / "web" / "icon.png"))
    draw(256).save(str(ROOT / "sshpro.ico"))
    print("icon written")

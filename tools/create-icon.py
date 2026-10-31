"""Generate a VG icon without reusing the CC Switch brand asset."""
from pathlib import Path
import sys
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QPointF, Qt
from PySide6.QtGui import QColor, QImage, QPainter, QPen

app = QApplication.instance() or QApplication(sys.argv)
root = Path(__file__).resolve().parents[1]
image = QImage(256, 256, QImage.Format.Format_ARGB32)
image.fill(Qt.GlobalColor.transparent)
painter = QPainter(image)
painter.setRenderHint(QPainter.RenderHint.Antialiasing)
painter.setPen(Qt.PenStyle.NoPen)
painter.setBrush(QColor("#f97316"))
painter.drawRoundedRect(8, 8, 240, 240, 54, 54)
pen = QPen(QColor("#ffffff"), 16)
pen.setCapStyle(Qt.PenCapStyle.RoundCap)
pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
painter.setPen(pen)
painter.drawLine(QPointF(65, 91), QPointF(104, 170))
painter.drawLine(QPointF(104, 170), QPointF(144, 91))
painter.drawArc(130, 86, 68, 88, 40 * 16, 280 * 16)
painter.drawLine(QPointF(181, 132), QPointF(202, 132))
painter.drawLine(QPointF(202, 132), QPointF(202, 161))
painter.end()
assert image.save(str(root / "src-tauri/icons/icon.ico"))
assert image.save(str(root / "src-tauri/icons/icon.png"))
print("VG icon created")

#include "ui/Icons.h"

#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QtMath>

namespace {

void arrowhead(QPainter &p, QPointF tip, double angleDeg, double len) {
    double a = qDegreesToRadians(angleDeg);
    QPointF d(qCos(a), qSin(a));
    QPointF n(-d.y(), d.x());
    QPointF base = tip - d * len;
    QPolygonF tri;
    tri << tip << base + n * (len * 0.6) << base - n * (len * 0.6);
    p.setBrush(p.pen().color());
    p.setPen(Qt::NoPen);
    p.drawPolygon(tri);
}

void drawUndo(QPainter &p, int s, bool redo) {
    double r = s * 0.27;
    QPointF c(s * 0.5, s * 0.54);
    QRectF box(c.x() - r, c.y() - r, 2 * r, 2 * r);
    QPainterPath path;
    if (!redo) {
        path.arcMoveTo(box, 150);
        path.arcTo(box, 150, -210);
    } else {
        path.arcMoveTo(box, 30);
        path.arcTo(box, 30, 210);
    }
    p.drawPath(path);
    double tipAngle = redo ? 30 : 150;
    double a = qDegreesToRadians(tipAngle);
    QPointF tip(c.x() + r * qCos(a), c.y() - r * qSin(a));
    arrowhead(p, tip + QPointF(0, redo ? -s * 0.02 : -s * 0.02),
              redo ? 250 : 290, s * 0.2);
}

void drawFlip(QPainter &p, int s) {
    double cx = s * 0.5;
    p.drawLine(QPointF(cx - s * 0.18, s * 0.5), QPointF(cx + s * 0.18, s * 0.5));
    QPolygonF up;
    up << QPointF(cx, s * 0.2) << QPointF(cx - s * 0.14, s * 0.4)
       << QPointF(cx + s * 0.14, s * 0.4);
    QPolygonF down;
    down << QPointF(cx, s * 0.8) << QPointF(cx - s * 0.14, s * 0.6)
         << QPointF(cx + s * 0.14, s * 0.6);
    p.setBrush(p.pen().color());
    QPen pen = p.pen();
    p.setPen(Qt::NoPen);
    p.drawPolygon(up);
    p.drawPolygon(down);
    p.setPen(pen);
}

void drawNew(QPainter &p, int s) {
    double c = s * 0.5;
    double r = s * 0.2;
    p.drawLine(QPointF(c, c - r), QPointF(c, c + r));
    p.drawLine(QPointF(c - r, c), QPointF(c + r, c));
}

void drawHint(QPainter &p, int s) {
    double cx = s * 0.5, cy = s * 0.42, r = s * 0.2;
    p.drawEllipse(QPointF(cx, cy), r, r);
    p.drawLine(QPointF(cx - r * 0.5, s * 0.68), QPointF(cx + r * 0.5, s * 0.68));
    p.drawLine(QPointF(cx - r * 0.35, s * 0.78), QPointF(cx + r * 0.35, s * 0.78));
}

void drawSettings(QPainter &p, int s) {
    double x0 = s * 0.22, x1 = s * 0.78;
    for (int i = 0; i < 3; ++i) {
        double y = s * (0.32 + i * 0.18);
        p.drawLine(QPointF(x0, y), QPointF(x1, y));
        double kx = x0 + (x1 - x0) * (i == 1 ? 0.32 : 0.68);
        p.setBrush(p.pen().color());
        QPen pen = p.pen();
        p.setPen(Qt::NoPen);
        p.drawEllipse(QPointF(kx, y), s * 0.055, s * 0.055);
        p.setPen(pen);
        p.setBrush(Qt::NoBrush);
    }
}

}

namespace Icons {

QIcon make(const QString &name, const QColor &color, int size) {
    QPixmap pm(size, size);
    pm.fill(Qt::transparent);
    QPainter p(&pm);
    p.setRenderHint(QPainter::Antialiasing, true);
    QPen pen(color, size * 0.085);
    pen.setCapStyle(Qt::RoundCap);
    pen.setJoinStyle(Qt::RoundJoin);
    p.setPen(pen);

    if (name == "undo") {
        drawUndo(p, size, false);
    } else if (name == "redo") {
        drawUndo(p, size, true);
    } else if (name == "flip") {
        drawFlip(p, size);
    } else if (name == "new") {
        drawNew(p, size);
    } else if (name == "hint") {
        drawHint(p, size);
    } else if (name == "settings") {
        drawSettings(p, size);
    }
    p.end();
    return QIcon(pm);
}

}

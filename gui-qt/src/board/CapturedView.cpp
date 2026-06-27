#include "board/CapturedView.h"

#include <QPainter>

namespace {
const char *kNames[6] = {"pawn", "knight", "bishop", "rook", "queen", "king"};
}

CapturedView::CapturedView(QWidget *parent) : QWidget(parent) {
    setFixedHeight(26);
    for (int idx = 0; idx < 12; ++idx) {
        QString color = idx < 6 ? "white" : "black";
        m_pieces[idx].load(QString(":/pieces/%1_%2.png").arg(color, kNames[idx % 6]));
    }
}

void CapturedView::setData(const QVector<int> &pieces, int advantage) {
    m_data = pieces;
    m_advantage = advantage;
    update();
}

void CapturedView::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.setRenderHint(QPainter::SmoothPixmapTransform, true);
    int sz = height() - 4;
    int x = 0;
    for (int idx : m_data) {
        if (idx >= 0 && idx < 12) {
            p.drawPixmap(QRect(x, 2, sz, sz), m_pieces[idx]);
            x += sz * 0.62;
        }
    }
    if (m_advantage > 0) {
        p.setPen(QColor(0x9a, 0xa0, 0xa8));
        QFont f = p.font();
        f.setPixelSize(int(height() * 0.5));
        f.setBold(true);
        p.setFont(f);
        p.drawText(QRect(x + sz / 2, 0, 50, height()), Qt::AlignVCenter | Qt::AlignLeft,
                   QString("+%1").arg(m_advantage));
    }
}

#include "board/BoardView.h"

#include <cmath>

#include <QMenu>
#include <QMouseEvent>
#include <QPainter>

#include "core/CoreBoard.h"

namespace {
const QColor kLight(0xEB, 0xEC, 0xD0);
const QColor kDark(0x73, 0x95, 0x52);
const QColor kLastMove(0xF6, 0xEB, 0x72, 160);
const QColor kSelected(0xF6, 0xEB, 0x72, 190);
const QColor kPremove(0xFF, 0x47, 0x47, 150);
const QColor kRedSquare(0xE0, 0x2B, 0x22, 190);
const QColor kArrow(0xFF, 0xA6, 0x29, 200);
const QColor kHint(0x4C, 0xAF, 0x50, 215);

const char *kNames[6] = {"pawn", "knight", "bishop", "rook", "queen", "king"};
}

BoardView::BoardView(QWidget *parent) : QWidget(parent) {
    setMinimumSize(360, 360);
    setMouseTracking(true);
    setFocusPolicy(Qt::StrongFocus);
    loadPieces();
}

void BoardView::loadPieces() {
    for (int idx = 0; idx < 12; ++idx) {
        QString color = idx < 6 ? "white" : "black";
        m_pieces[idx].load(QString(":/pieces/%1_%2.png").arg(color, kNames[idx % 6]));
    }
}

void BoardView::setBoard(CoreBoard *board) {
    m_board = board;
    update();
}

void BoardView::setPremoveBoard(CoreBoard *board) { m_pmBoard = board; }

CoreBoard *BoardView::renderBoard() const {
    return (!m_premoves.isEmpty() && m_pmBoard) ? m_pmBoard : m_board;
}

void BoardView::setInteractive(bool on) { m_interactive = on; }

void BoardView::setFlipped(bool on) {
    m_flipped = on;
    update();
}

void BoardView::setHumanColors(bool whiteHuman, bool blackHuman) {
    m_humanMask = (whiteHuman ? 1 : 0) | (blackHuman ? 2 : 0);
}

void BoardView::setPremoveEnabled(bool on) {
    m_premoveEnabled = on;
    if (!on) {
        if (m_selPremove) {
            clearSelection();
        }
        m_premoves.clear();
        update();
    }
}

void BoardView::setPremoves(const QVector<QPair<int, int>> &moves) {
    m_premoves.clear();
    for (const auto &m : moves) {
        m_premoves.append(m);
    }
    update();
}

void BoardView::setLastMove(int from, int to) {
    m_lastFrom = from;
    m_lastTo = to;
    m_hintFrom = -1;
    m_hintTo = -1;
    update();
}

void BoardView::setHint(int from, int to) {
    m_hintFrom = from;
    m_hintTo = to;
    update();
}

void BoardView::clearSelection() {
    m_selected = -1;
    m_selPremove = false;
    m_targets.clear();
    m_dragFrom = -1;
    m_dragging = false;
    update();
}

void BoardView::clearAnnotations() {
    m_redSquares.clear();
    m_arrows.clear();
    update();
}

void BoardView::refresh() { update(); }

qreal BoardView::cellSize() const { return qMin(width(), height()) / 8.0; }

QRectF BoardView::squareRect(int square) const {
    qreal cell = cellSize();
    int side = int(cell * 8);
    qreal ox = (width() - side) / 2.0;
    qreal oy = (height() - side) / 2.0;
    int file = square & 7;
    int rank = square >> 3;
    int col = m_flipped ? 7 - file : file;
    int row = m_flipped ? rank : 7 - rank;
    return QRectF(ox + col * cell, oy + row * cell, cell, cell);
}

int BoardView::squareAt(const QPointF &pos) const {
    qreal cell = cellSize();
    int side = int(cell * 8);
    qreal ox = (width() - side) / 2.0;
    qreal oy = (height() - side) / 2.0;
    if (pos.x() < ox || pos.y() < oy || pos.x() >= ox + side || pos.y() >= oy + side) {
        return -1;
    }
    int col = int((pos.x() - ox) / cell);
    int row = int((pos.y() - oy) / cell);
    if (col < 0 || col > 7 || row < 0 || row > 7) {
        return -1;
    }
    int file = m_flipped ? 7 - col : col;
    int rank = m_flipped ? row : 7 - row;
    return rank * 8 + file;
}

bool BoardView::normalAllowed(int color) const {
    return m_board && color == m_board->sideToMove() && (m_humanMask & (1 << color));
}

bool BoardView::premoveAllowed(int color) const {
    if (!m_board || !m_premoveEnabled) {
        return false;
    }
    int stm = m_board->sideToMove();
    return (m_humanMask & (1 << color)) && color != stm && !(m_humanMask & (1 << stm));
}

void BoardView::drawArrow(QPainter &p, int from, int to, const QColor &color, qreal cell) {
    QPointF a = squareRect(from).center();
    QPointF b = squareRect(to).center();
    QPointF d = b - a;
    qreal len = std::hypot(d.x(), d.y());
    if (len < 1) {
        return;
    }
    d /= len;
    QPointF n(-d.y(), d.x());
    QPointF end = b - d * (cell * 0.34);
    p.setPen(QPen(color, cell * 0.16, Qt::SolidLine, Qt::RoundCap));
    p.setBrush(Qt::NoBrush);
    p.drawLine(a + d * (cell * 0.16), end);
    qreal h = cell * 0.34;
    QPolygonF head;
    head << b << (b - d * h + n * (h * 0.55)) << (b - d * h - n * (h * 0.55));
    p.setPen(Qt::NoPen);
    p.setBrush(color);
    p.drawPolygon(head);
}

void BoardView::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    p.setRenderHint(QPainter::SmoothPixmapTransform, true);
    qreal cell = cellSize();

    for (int sq = 0; sq < 64; ++sq) {
        bool dark = ((sq & 7) + (sq >> 3)) % 2 == 0;
        p.fillRect(squareRect(sq), dark ? kDark : kLight);
    }
    if (m_lastFrom >= 0) {
        p.fillRect(squareRect(m_lastFrom), kLastMove);
        p.fillRect(squareRect(m_lastTo), kLastMove);
    }
    for (const auto &pm : m_premoves) {
        p.fillRect(squareRect(pm.first), kPremove);
        p.fillRect(squareRect(pm.second), kPremove);
    }
    if (m_selected >= 0) {
        p.fillRect(squareRect(m_selected), kSelected);
    }
    for (int sq : m_redSquares) {
        p.fillRect(squareRect(sq), kRedSquare);
    }

    QFont coord = p.font();
    coord.setPixelSize(int(cell * 0.18));
    coord.setBold(true);
    p.setFont(coord);
    for (int sq = 0; sq < 64; ++sq) {
        int file = sq & 7;
        int rank = sq >> 3;
        int col = m_flipped ? 7 - file : file;
        int row = m_flipped ? rank : 7 - rank;
        bool dark = (file + rank) % 2 == 0;
        QRectF r = squareRect(sq);
        p.setPen(dark ? kLight : kDark);
        if (row == 7) {
            p.drawText(r.adjusted(0, 0, -cell * 0.06, -cell * 0.02),
                       Qt::AlignRight | Qt::AlignBottom, QString(QChar('a' + file)));
        }
        if (col == 0) {
            p.drawText(r.adjusted(cell * 0.06, cell * 0.02, 0, 0),
                       Qt::AlignLeft | Qt::AlignTop, QString(QChar('1' + rank)));
        }
    }

    if (!m_board) {
        return;
    }

    CoreBoard *rb = renderBoard();
    for (int to : m_targets) {
        QRectF r = squareRect(to);
        bool capture = rb->pieceAt(to) >= 0;
        p.setPen(Qt::NoPen);
        if (capture) {
            qreal pen = cell * 0.08;
            p.setBrush(Qt::NoBrush);
            p.setPen(QPen(QColor(20, 20, 20, 60), pen));
            p.drawEllipse(r.adjusted(pen, pen, -pen, -pen));
            p.setPen(Qt::NoPen);
        } else {
            p.setBrush(QColor(20, 20, 20, 45));
            qreal d = cell * 0.3;
            p.drawEllipse(r.center(), d / 2, d / 2);
        }
    }

    for (int sq = 0; sq < 64; ++sq) {
        if (m_dragging && sq == m_dragFrom) {
            continue;
        }
        int piece = rb->pieceAt(sq);
        if (piece < 0) {
            continue;
        }
        QRectF r = squareRect(sq);
        qreal m = cell * 0.06;
        p.drawPixmap(r.adjusted(m, m, -m, -m).toRect(), m_pieces[piece]);
    }

    for (const auto &arrow : m_arrows) {
        drawArrow(p, arrow.first, arrow.second, kArrow, cell);
    }
    if (m_hintFrom >= 0 && m_hintTo >= 0) {
        drawArrow(p, m_hintFrom, m_hintTo, kHint, cell);
    }

    if (m_dragging && m_dragFrom >= 0) {
        int piece = rb->pieceAt(m_dragFrom);
        if (piece >= 0) {
            qreal s = cell * 0.92;
            p.drawPixmap(QRectF(m_dragPos.x() - s / 2, m_dragPos.y() - s / 2, s, s).toRect(),
                         m_pieces[piece]);
        }
    }
}

int BoardView::promotionChoice(int to) {
    QMenu menu(this);
    QAction *q = menu.addAction(tr("Queen"));
    QAction *r = menu.addAction(tr("Rook"));
    QAction *b = menu.addAction(tr("Bishop"));
    QAction *n = menu.addAction(tr("Knight"));
    QAction *picked = menu.exec(mapToGlobal(squareRect(to).center().toPoint()));
    if (!picked) return -1;
    if (picked == r) return 3;
    if (picked == b) return 2;
    if (picked == n) return 1;
    return 4;
}

void BoardView::selectSquare(int sq, bool premove) {
    m_selected = sq;
    m_selPremove = premove;
    m_targets = premove ? renderBoard()->premoveTargets(sq) : renderBoard()->legalTo(sq);
}

void BoardView::commitSelection(int to) {
    int from = m_selected;
    bool premove = m_selPremove;
    int promo = 0;
    int piece = renderBoard()->pieceAt(from);
    bool isPawn = piece >= 0 && piece % 6 == 0;
    int toRank = to >> 3;
    if (isPawn && (toRank == 7 || toRank == 0)) {
        promo = promotionChoice(to);
        if (promo < 0) {
            clearSelection();
            return;
        }
    }
    if (premove) {
        emit premoveRequested(from, to, promo);
    } else {
        emit moveRequested(from, to, promo);
    }
    clearSelection();
}

void BoardView::mousePressEvent(QMouseEvent *event) {
    setFocus();
    if (!m_board) {
        return;
    }
    if (event->button() == Qt::RightButton) {
        m_rmFrom = squareAt(event->position());
        return;
    }
    if (event->button() != Qt::LeftButton || !m_interactive) {
        return;
    }
    if (!m_redSquares.isEmpty() || !m_arrows.isEmpty()) {
        clearAnnotations();
    }

    int sq = squareAt(event->position());
    if (sq < 0) {
        clearSelection();
        return;
    }

    if (m_selected >= 0 && m_targets.contains(sq)) {
        commitSelection(sq);
        return;
    }

    int piece = renderBoard()->pieceAt(sq);
    int c = piece < 6 ? 0 : 1;
    if (piece >= 0 && normalAllowed(c)) {
        selectSquare(sq, false);
        m_dragFrom = sq;
        m_dragging = true;
        m_dragPos = event->position();
    } else if (piece >= 0 && premoveAllowed(c)) {
        selectSquare(sq, true);
        m_dragFrom = sq;
        m_dragging = true;
        m_dragPos = event->position();
    } else {
        clearSelection();
        if (!m_premoves.isEmpty()) {
            emit premoveRequested(-1, -1, 0);
        }
    }
    update();
}

void BoardView::mouseMoveEvent(QMouseEvent *event) {
    if (m_dragging) {
        m_dragPos = event->position();
        update();
    }
}

void BoardView::mouseReleaseEvent(QMouseEvent *event) {
    if (event->button() == Qt::RightButton) {
        if (m_rmFrom >= 0) {
            int to = squareAt(event->position());
            if (to < 0 || to == m_rmFrom) {
                if (m_redSquares.contains(m_rmFrom)) {
                    m_redSquares.remove(m_rmFrom);
                } else {
                    m_redSquares.insert(m_rmFrom);
                }
            } else {
                QPair<int, int> a(m_rmFrom, to);
                if (m_arrows.contains(a)) {
                    m_arrows.removeAll(a);
                } else {
                    m_arrows.append(a);
                }
            }
            m_rmFrom = -1;
            update();
        }
        return;
    }

    if (!m_dragging) {
        return;
    }
    m_dragging = false;
    int to = squareAt(event->position());
    if (m_selected >= 0 && to >= 0 && to != m_dragFrom && m_targets.contains(to)) {
        commitSelection(to);
        return;
    }
    update();
}

#pragma once

#include <QList>
#include <QPair>
#include <QPixmap>
#include <QPointF>
#include <QSet>
#include <QVector>
#include <QWidget>

class CoreBoard;

class BoardView : public QWidget {
    Q_OBJECT
public:
    explicit BoardView(QWidget *parent = nullptr);

    void setBoard(CoreBoard *board);
    void setPremoveBoard(CoreBoard *board);
    void setInteractive(bool on);
    void setFlipped(bool on);
    bool flipped() const { return m_flipped; }
    void setLastMove(int from, int to);
    void setHint(int from, int to);
    void setHumanColors(bool whiteHuman, bool blackHuman);
    void setPremoveEnabled(bool on);
    void setPremoves(const QVector<QPair<int, int>> &moves);
    void clearSelection();
    void clearAnnotations();
    void refresh();

    QSize sizeHint() const override { return QSize(640, 640); }
    int heightForWidth(int w) const override { return w; }
    bool hasHeightForWidth() const override { return true; }

signals:
    void moveRequested(int from, int to, int promo);
    void premoveRequested(int from, int to, int promo);

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;

private:
    int squareAt(const QPointF &pos) const;
    QRectF squareRect(int square) const;
    qreal cellSize() const;
    CoreBoard *renderBoard() const;
    int promotionChoice(int to);
    void loadPieces();
    void selectSquare(int sq, bool premove);
    void commitSelection(int to);
    bool normalAllowed(int color) const;
    bool premoveAllowed(int color) const;
    void drawArrow(QPainter &p, int from, int to, const QColor &color, qreal cell);

    CoreBoard *m_board = nullptr;
    CoreBoard *m_pmBoard = nullptr;
    QPixmap m_pieces[12];
    bool m_interactive = true;
    bool m_flipped = false;
    int m_humanMask = 3;
    bool m_premoveEnabled = true;

    int m_selected = -1;
    bool m_selPremove = false;
    QVector<int> m_targets;

    int m_lastFrom = -1;
    int m_lastTo = -1;
    int m_hintFrom = -1;
    int m_hintTo = -1;
    QList<QPair<int, int>> m_premoves;

    int m_dragFrom = -1;
    bool m_dragging = false;
    QPointF m_dragPos;

    int m_rmFrom = -1;
    QSet<int> m_redSquares;
    QList<QPair<int, int>> m_arrows;
};

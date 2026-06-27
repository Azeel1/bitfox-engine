#pragma once

#include <QPixmap>
#include <QVector>
#include <QWidget>

class CapturedView : public QWidget {
    Q_OBJECT
public:
    explicit CapturedView(QWidget *parent = nullptr);
    void setData(const QVector<int> &pieces, int advantage);
    QSize sizeHint() const override { return QSize(240, 26); }

protected:
    void paintEvent(QPaintEvent *event) override;

private:
    QPixmap m_pieces[12];
    QVector<int> m_data;
    int m_advantage = 0;
};

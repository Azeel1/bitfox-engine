#pragma once

#include <QStringList>
#include <QVector>
#include <QWidget>

#include "engine/EngineCatalog.h"

class QComboBox;
class QSpinBox;
class QPushButton;
class QListWidget;
class QLabel;
class CapturedView;

class SidePanel : public QWidget {
    Q_OBJECT
public:
    explicit SidePanel(QWidget *parent = nullptr);

    void setEngines(const QVector<EngineEntry> &engines);
    void rebuildMoves(const QStringList &moves);
    void clearMoves();
    void setStatus(const QString &text);
    void setEval(const QString &name, int depth, int scoreCp);
    void setCaptured(const QVector<int> &whiteTook, int whiteAdv,
                     const QVector<int> &blackTook, int blackAdv);
    int movetime() const;

signals:
    void engineSelected(int color, const QString &path);
    void movetimeChanged(int ms);
    void newGameRequested();
    void flipRequested();
    void undoRequested();
    void redoRequested();
    void hintRequested();
    void settingsRequested();

private:
    QComboBox *m_white;
    QComboBox *m_black;
    QSpinBox *m_time;
    QPushButton *m_new;
    QPushButton *m_flip;
    QPushButton *m_undo;
    QPushButton *m_redo;
    QPushButton *m_hint;
    QPushButton *m_settings;
    QListWidget *m_moves;
    QLabel *m_status;
    QLabel *m_eval;
    CapturedView *m_capByWhite;
    CapturedView *m_capByBlack;
};

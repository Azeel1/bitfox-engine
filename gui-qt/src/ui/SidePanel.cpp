#include "ui/SidePanel.h"

#include <QComboBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QLocale>
#include <QPushButton>
#include <QSpinBox>
#include <QVBoxLayout>

#include "board/CapturedView.h"
#include "ui/Icons.h"

static QPushButton *iconButton(const QString &icon, const QString &tip, QWidget *parent) {
    auto *b = new QPushButton(parent);
    b->setObjectName("iconbtn");
    b->setIcon(Icons::make(icon));
    b->setIconSize(QSize(22, 22));
    b->setFixedSize(40, 38);
    b->setToolTip(tip);
    b->setCursor(Qt::PointingHandCursor);
    return b;
}

SidePanel::SidePanel(QWidget *parent) : QWidget(parent) {
    setObjectName("sidePanel");
    setFixedWidth(300);

    auto *root = new QVBoxLayout(this);
    root->setContentsMargins(18, 18, 18, 18);
    root->setSpacing(12);

    auto *title = new QLabel("Bitfox", this);
    title->setObjectName("appTitle");
    root->addWidget(title);

    m_new = new QPushButton(tr("New game"), this);
    m_new->setObjectName("primary");
    m_new->setIcon(Icons::make("new", QColor(0x14, 0x21, 0x0a), 24));
    m_new->setIconSize(QSize(16, 16));
    m_new->setCursor(Qt::PointingHandCursor);
    root->addWidget(m_new);

    auto *icons = new QHBoxLayout();
    icons->setSpacing(8);
    m_flip = iconButton("flip", tr("Flip board (F)"), this);
    m_undo = iconButton("undo", tr("Undo (U)"), this);
    m_redo = iconButton("redo", tr("Redo (R)"), this);
    m_hint = iconButton("hint", tr("Hint - best move (H)"), this);
    m_settings = iconButton("settings", tr("Settings"), this);
    icons->addWidget(m_flip);
    icons->addWidget(m_undo);
    icons->addWidget(m_redo);
    icons->addWidget(m_hint);
    icons->addWidget(m_settings);
    root->addLayout(icons);

    auto *players = new QGroupBox(tr("Settings"), this);
    auto *form = new QFormLayout(players);
    form->setSpacing(8);
    m_white = new QComboBox(players);
    m_black = new QComboBox(players);
    form->addRow(tr("White"), m_white);
    form->addRow(tr("Black"), m_black);
    m_time = new QSpinBox(players);
    m_time->setLocale(QLocale(QLocale::English, QLocale::UnitedStates));
    m_time->setRange(50, 60000);
    m_time->setSingleStep(100);
    m_time->setValue(1000);
    m_time->setSuffix(tr(" ms"));
    form->addRow(tr("Think time"), m_time);
    root->addWidget(players);

    m_capByBlack = new CapturedView(this);
    m_capByWhite = new CapturedView(this);
    root->addWidget(m_capByBlack);
    root->addWidget(m_capByWhite);

    m_moves = new QListWidget(this);
    m_moves->setObjectName("moveList");
    root->addWidget(m_moves, 1);

    m_eval = new QLabel(tr("-"), this);
    m_eval->setObjectName("eval");
    root->addWidget(m_eval);

    m_status = new QLabel(tr("Ready"), this);
    m_status->setObjectName("status");
    m_status->setWordWrap(true);
    root->addWidget(m_status);

    connect(m_new, &QPushButton::clicked, this, &SidePanel::newGameRequested);
    connect(m_flip, &QPushButton::clicked, this, &SidePanel::flipRequested);
    connect(m_undo, &QPushButton::clicked, this, &SidePanel::undoRequested);
    connect(m_redo, &QPushButton::clicked, this, &SidePanel::redoRequested);
    connect(m_hint, &QPushButton::clicked, this, &SidePanel::hintRequested);
    connect(m_settings, &QPushButton::clicked, this, &SidePanel::settingsRequested);
    connect(m_time, qOverload<int>(&QSpinBox::valueChanged), this,
            &SidePanel::movetimeChanged);
    connect(m_white, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this](int) { emit engineSelected(0, m_white->currentData().toString()); });
    connect(m_black, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this](int) { emit engineSelected(1, m_black->currentData().toString()); });
}

void SidePanel::setEngines(const QVector<EngineEntry> &engines) {
    for (QComboBox *combo : {m_white, m_black}) {
        combo->blockSignals(true);
        combo->clear();
        combo->addItem(tr("Human"), QString());
        for (const EngineEntry &e : engines) {
            combo->addItem(e.name, e.path);
        }
        combo->blockSignals(false);
    }
    if (!engines.isEmpty()) {
        m_black->setCurrentIndex(1);
    }
}

void SidePanel::rebuildMoves(const QStringList &moves) {
    m_moves->clear();
    for (int i = 0; i < moves.size(); ++i) {
        if (i % 2 == 0) {
            m_moves->addItem(QString("%1. %2").arg(i / 2 + 1).arg(moves[i]));
        } else {
            QListWidgetItem *last = m_moves->item(m_moves->count() - 1);
            last->setText(last->text() + "   " + moves[i]);
        }
    }
    m_moves->scrollToBottom();
}

void SidePanel::clearMoves() { m_moves->clear(); }

void SidePanel::setStatus(const QString &text) { m_status->setText(text); }

void SidePanel::setEval(const QString &name, int depth, int scoreCp) {
    QString cp;
    if (scoreCp >= 100000) {
        cp = "#";
    } else if (scoreCp <= -100000) {
        cp = "-#";
    } else {
        QString sign = scoreCp > 0 ? "+" : "";
        cp = QString("%1%2").arg(sign).arg(scoreCp / 100.0, 0, 'f', 2);
    }
    m_eval->setText(tr("%1   depth %2   eval %3").arg(name).arg(depth).arg(cp));
}

void SidePanel::setCaptured(const QVector<int> &whiteTook, int whiteAdv,
                            const QVector<int> &blackTook, int blackAdv) {
    m_capByWhite->setData(whiteTook, whiteAdv);
    m_capByBlack->setData(blackTook, blackAdv);
}

int SidePanel::movetime() const { return m_time->value(); }

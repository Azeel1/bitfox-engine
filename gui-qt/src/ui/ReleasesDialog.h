#pragma once

#include <QDialog>
#include <QVector>

#include "engine/ReleaseManager.h"

class QListWidget;
class QLabel;
class QPushButton;

class ReleasesDialog : public QDialog {
    Q_OBJECT
public:
    explicit ReleasesDialog(QWidget *parent = nullptr);

signals:
    void enginesChanged();

private:
    ReleaseManager *m_manager;
    QListWidget *m_list;
    QLabel *m_status;
    QVector<ReleaseInfo> m_releases;
    bool m_changed = false;

    void reload();
    void download(int row);
};

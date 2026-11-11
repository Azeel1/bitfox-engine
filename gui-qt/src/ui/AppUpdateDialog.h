#pragma once

#include <QDialog>

class AppUpdater;
class QLabel;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;

class AppUpdateDialog : public QDialog {
    Q_OBJECT
public:
    explicit AppUpdateDialog(AppUpdater *updater, QWidget *parent = nullptr);

private:
    void refresh();
    AppUpdater *m_updater;
    QLabel *m_status;
    QPlainTextEdit *m_notes;
    QProgressBar *m_progress;
    QPushButton *m_check;
    QPushButton *m_update;
    QPushButton *m_open;
    QPushButton *m_later;
};

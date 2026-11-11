#include "ui/AppUpdateDialog.h"

#include <QCoreApplication>
#include <QDesktopServices>
#include <QHBoxLayout>
#include <QLabel>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QProgressBar>
#include <QPushButton>
#include <QUrl>
#include <QVBoxLayout>

#include "update/AppUpdater.h"

AppUpdateDialog::AppUpdateDialog(AppUpdater *updater, QWidget *parent)
    : QDialog(parent), m_updater(updater) {
    setWindowTitle(tr("Bitfox Board application update"));
    setMinimumSize(520, 390);
    auto *layout = new QVBoxLayout(this);
    layout->addWidget(new QLabel(tr("Installed application: %1").arg(QCoreApplication::applicationVersion()), this));
    m_status = new QLabel(this);
    m_status->setTextFormat(Qt::PlainText);
    m_status->setWordWrap(true);
    layout->addWidget(m_status);
    m_notes = new QPlainTextEdit(this);
    m_notes->setReadOnly(true);
    m_notes->setAccessibleName(tr("Release changes"));
    layout->addWidget(m_notes, 1);
    auto *instructions = new QLabel(tr("The update downloads the full desktop application. After verification, open the archive, extract it, and replace the installed application when you are ready. Your running game stays open."), this);
    instructions->setWordWrap(true);
    layout->addWidget(instructions);
    m_progress = new QProgressBar(this);
    m_progress->setRange(0, 100);
    m_progress->setAccessibleName(tr("Application download progress"));
    layout->addWidget(m_progress);
    auto *buttons = new QHBoxLayout;
    m_check = new QPushButton(tr("Check again"), this);
    m_update = new QPushButton(tr("Update now"), this);
    m_update->setObjectName("primary");
    m_open = new QPushButton(tr("Open verified package"), this);
    m_later = new QPushButton(this);
    buttons->addWidget(m_check);
    buttons->addStretch();
    buttons->addWidget(m_update);
    buttons->addWidget(m_open);
    buttons->addWidget(m_later);
    layout->addLayout(buttons);
    connect(m_check, &QPushButton::clicked, updater, [updater] { updater->check(true); });
    connect(m_update, &QPushButton::clicked, updater, &AppUpdater::download);
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (!QDesktopServices::openUrl(QUrl::fromLocalFile(m_updater->packagePath()))) {
            QMessageBox::warning(this, tr("Could not open package"),
                                 tr("Open the verified archive manually at:\n%1").arg(m_updater->packagePath()));
            return;
        }
        accept();
    });
    connect(m_later, &QPushButton::clicked, this, [this] {
        if (m_updater->busy()) m_updater->cancel();
        else if (m_updater->state() == AppUpdater::Available) {
            m_updater->defer();
            reject();
        }
        else reject();
    });
    connect(this, &QDialog::rejected, updater, [updater] {
        if (updater->busy()) updater->cancel();
        else if (updater->state() == AppUpdater::Available) updater->defer();
    });
    connect(updater, &AppUpdater::stateChanged, this, &AppUpdateDialog::refresh);
    connect(updater, &AppUpdater::progress, this, [this](qint64 received, qint64 total) {
        m_progress->setRange(0, 100);
        m_progress->setValue(total > 0 ? int(received * 100 / total) : 0);
    });
    refresh();
}

void AppUpdateDialog::refresh() {
    const auto state = m_updater->state();
    m_status->setText(m_updater->message().isEmpty() ? tr("Check for a newer desktop application.") : m_updater->message());
    m_notes->setPlainText(m_updater->release().notes);
    m_notes->setVisible(!m_updater->release().notes.isEmpty());
    m_check->setEnabled(!m_updater->busy());
    const bool retry = !m_updater->busy() && !m_updater->release().assetName.isEmpty() && state != AppUpdater::Ready;
    m_update->setVisible(state == AppUpdater::Available || retry);
    m_open->setVisible(state == AppUpdater::Ready);
    m_later->setText(m_updater->busy() ? tr("Cancel") : state == AppUpdater::Available ? tr("Later") : tr("Close"));
    m_progress->setVisible(m_updater->busy());
    if (m_updater->busy()) m_progress->setRange(0, 0);
}

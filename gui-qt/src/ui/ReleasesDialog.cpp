#include "ui/ReleasesDialog.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QListWidgetItem>
#include <QPushButton>
#include <QVBoxLayout>

ReleasesDialog::ReleasesDialog(QWidget *parent) : QDialog(parent) {
    setWindowTitle(tr("Bitfox versions"));
    setMinimumSize(420, 360);

    auto *lay = new QVBoxLayout(this);
    m_status = new QLabel(tr("Loading releases..."), this);
    lay->addWidget(m_status);
    m_list = new QListWidget(this);
    lay->addWidget(m_list, 1);
    auto *close = new QPushButton(tr("Close"), this);
    close->setObjectName("primary");
    lay->addWidget(close);
    connect(close, &QPushButton::clicked, this, &QDialog::accept);

    m_manager = new ReleaseManager(this);
    connect(m_manager, &ReleaseManager::releasesReady, this,
            [this](const QVector<ReleaseInfo> &releases) {
                m_releases = releases;
                m_status->setText(releases.isEmpty()
                                       ? tr("No downloadable builds found for this platform.")
                                       : tr("Available versions:"));
                m_list->clear();
                for (int i = 0; i < m_releases.size(); ++i) {
                    const ReleaseInfo &r = m_releases[i];
                    auto *row = new QWidget(m_list);
                    auto *h = new QHBoxLayout(row);
                    h->setContentsMargins(6, 4, 6, 4);
                    auto *label = new QLabel(r.name, row);
                    h->addWidget(label, 1);
                    if (r.installed) {
                        auto *installed = new QLabel(tr("Installed"), row);
                        installed->setStyleSheet("color:#81b64c; font-weight:600;");
                        h->addWidget(installed);
                    } else {
                        auto *btn = new QPushButton(tr("Download"), row);
                        connect(btn, &QPushButton::clicked, this, [this, i, btn] {
                            btn->setEnabled(false);
                            download(i);
                        });
                        h->addWidget(btn);
                    }
                    auto *item = new QListWidgetItem(m_list);
                    item->setSizeHint(row->sizeHint());
                    m_list->addItem(item);
                    m_list->setItemWidget(item, row);
                }
            });
    connect(m_manager, &ReleaseManager::fetchFailed, this, [this](const QString &error) {
        m_status->setText(tr("Could not reach GitHub: %1").arg(error));
    });
    connect(m_manager, &ReleaseManager::downloadProgress, this,
            [this](const QString &tag, qint64 received, qint64 total) {
                if (total > 0) {
                    m_status->setText(
                        tr("Downloading %1... %2%").arg(tag).arg(received * 100 / total));
                }
            });
    connect(m_manager, &ReleaseManager::downloadFinished, this,
            [this](const QString &tag, bool success, const QString &error) {
                if (success) {
                    m_status->setText(tr("%1 installed.").arg(tag));
                    m_changed = true;
                    emit enginesChanged();
                    reload();
                } else {
                    m_status->setText(tr("Failed to install %1: %2").arg(tag, error));
                }
            });

    reload();
}

void ReleasesDialog::reload() {
    m_status->setText(tr("Loading releases..."));
    m_manager->fetchReleases();
}

void ReleasesDialog::download(int row) {
    if (row < 0 || row >= m_releases.size()) {
        return;
    }
    m_status->setText(tr("Downloading %1...").arg(m_releases[row].tag));
    m_manager->downloadRelease(m_releases[row]);
}

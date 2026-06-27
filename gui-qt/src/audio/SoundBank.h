#pragma once

#include <QHash>
#include <QObject>
#include <QString>

class QSoundEffect;

class SoundBank : public QObject {
    Q_OBJECT
public:
    explicit SoundBank(QObject *parent = nullptr);
    void play(const QString &key);
    void setMuted(bool muted) { m_muted = muted; }

private:
    void add(const QString &key, const QString &resource);
    QHash<QString, QSoundEffect *> m_effects;
    bool m_muted = false;
};

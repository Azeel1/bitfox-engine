#include "audio/SoundBank.h"

#include <QSoundEffect>
#include <QUrl>

SoundBank::SoundBank(QObject *parent) : QObject(parent) {
    add("move", "qrc:/sounds/move.wav");
    add("capture", "qrc:/sounds/capture.wav");
    add("castle", "qrc:/sounds/castle.wav");
    add("check", "qrc:/sounds/check.wav");
    add("promote", "qrc:/sounds/promote.wav");
    add("end", "qrc:/sounds/end.wav");
    add("illegal", "qrc:/sounds/illegal.wav");
}

void SoundBank::add(const QString &key, const QString &resource) {
    auto *effect = new QSoundEffect(this);
    effect->setSource(QUrl(resource));
    effect->setVolume(0.6);
    m_effects.insert(key, effect);
}

void SoundBank::play(const QString &key) {
    if (m_muted) {
        return;
    }
    if (QSoundEffect *effect = m_effects.value(key, nullptr)) {
        effect->play();
    }
}

#include "engine/EngineCatalog.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QRegularExpression>
#include <QSet>

#include <algorithm>

#include "engine/ReleaseManager.h"
#include "update/AppUpdater.h"

struct EngineVersion {
    QStringList core;
    QString semantic;
    bool prerelease = false;
};

static EngineVersion versionOf(QString name) {
    name = name.trimmed();
    if (name.endsWith(".exe", Qt::CaseInsensitive)) {
        name.chop(4);
    }

    static const QRegularExpression semantic(
        QStringLiteral("(?<![0-9.])(([0-9]+\\.[0-9]+\\.[0-9]+)(?:-[0-9A-Za-z.-]+)?(?:\\+[0-9A-Za-z.-]+)?)$"));
    const auto match = semantic.match(name);
    if (match.hasMatch()) {
        bool valid = false;
        AppUpdater::compareVersions(match.captured(1), match.captured(1), &valid);
        if (valid) {
            return {
                match.captured(2).split('.'),
                match.captured(1),
                match.captured(1).mid(match.captured(2).size()).startsWith('-'),
            };
        }
    }

    static const QRegularExpression re(QStringLiteral("(\\d+(?:\\.\\d+)*)$"));
    const QRegularExpressionMatch m = re.match(name);
    return {m.hasMatch() ? m.captured(1).split('.') : QStringList(), {}, false};
}

static QString numericPart(QString value) {
    qsizetype first = 0;
    while (first + 1 < value.size() && value[first] == '0') {
        ++first;
    }
    return value.mid(first);
}

static bool newerThan(const EngineEntry &a, const EngineEntry &b) {
    const EngineVersion va = versionOf(a.name);
    const EngineVersion vb = versionOf(b.name);
    const qsizetype count = qMax(va.core.size(), vb.core.size());

    for (qsizetype i = 0; i < count; ++i) {
        const QString x = numericPart(i < va.core.size() ? va.core[i] : QStringLiteral("0"));
        const QString y = numericPart(i < vb.core.size() ? vb.core[i] : QStringLiteral("0"));
        if (x.size() != y.size()) {
            return x.size() > y.size();
        }
        const int result = QString::compare(x, y, Qt::CaseSensitive);
        if (result != 0) {
            return result > 0;
        }
    }

    if (va.prerelease != vb.prerelease) {
        return !va.prerelease;
    }
    if (va.prerelease) {
        const int result = AppUpdater::compareVersions(va.semantic, vb.semantic);
        if (result != 0) {
            return result > 0;
        }
    }
    return a.name < b.name;
}

bool EngineCatalog::looksLikeEngine(const QString &path) {
    QFileInfo info(path);
    if (!info.isFile() || !info.isExecutable()) {
        return false;
    }
    static const QSet<QString> skip = {"dylib", "so",  "dll", "rlib",
                                       "d",     "rmeta", "a",   "txt"};
    return !skip.contains(info.suffix().toLower());
}

QVector<EngineEntry> EngineCatalog::scanDir(const QString &dir) {
    QVector<EngineEntry> found;
    QDir d(dir);
    if (!d.exists()) {
        return found;
    }
    QSet<QString> packagedNames;
#if defined(Q_OS_WIN)
    const QFileInfoList folders = d.entryInfoList(QDir::Dirs | QDir::NoDotAndDotDot);
    for (const QFileInfo &folder : folders) {
        const EngineVersion version = versionOf(folder.fileName() + ".exe");
        if (folder.isSymLink() || version.semantic.isEmpty() ||
            folder.fileName() != "Bitfox " + version.semantic) continue;
        const QDir package(folder.absoluteFilePath());
        const QFileInfo binary(package.filePath(folder.fileName() + ".exe"));
        const QFileInfo manifest(package.filePath("RUNTIME_DEPENDENCIES.json"));
        if (binary.isSymLink() || !looksLikeEngine(binary.absoluteFilePath()) ||
            !manifest.isFile() || manifest.isSymLink()) continue;
        found.append({binary.fileName(), binary.absoluteFilePath()});
        packagedNames.insert(binary.fileName());
    }
#endif
    const QFileInfoList entries = d.entryInfoList(QDir::Files | QDir::NoDotAndDotDot);
    for (const QFileInfo &info : entries) {
        if (!packagedNames.contains(info.fileName()) && looksLikeEngine(info.absoluteFilePath())) {
            found.append({info.fileName(), info.absoluteFilePath()});
        }
    }
    return found;
}

QVector<EngineEntry> EngineCatalog::discover(const QStringList &directories) {
    QVector<EngineEntry> all;
    QSet<QString> seen;

    QStringList roots = directories;
    if (roots.isEmpty()) {
        const QString appDir = QCoreApplication::applicationDirPath();
        roots << ReleaseManager::enginesDir();
        roots << appDir + "/engines";
        roots << appDir + "/../engines";
    }

    for (const QString &root : roots) {
        for (const EngineEntry &e : scanDir(root)) {
            if (!seen.contains(e.path)) {
                seen.insert(e.path);
                all.append(e);
            }
        }
    }
    std::sort(all.begin(), all.end(), newerThan);
    return all;
}

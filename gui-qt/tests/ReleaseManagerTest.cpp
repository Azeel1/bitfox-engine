#include <QDir>
#include <QCoreApplication>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QCryptographicHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPointer>
#include <QProcess>
#include <QProcessEnvironment>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QUuid>
#include <QtTest>

#if !defined(Q_OS_WIN)
#include <sys/stat.h>
#else
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <tlhelp32.h>
#endif

#include "engine/ReleaseManager.h"

namespace {
bool runProcess(const QString &program, const QStringList &arguments,
                const QString &output = QString()) {
    QProcess process;
    if (!output.isEmpty()) {
        process.setStandardOutputFile(output, QIODevice::Truncate);
    }
    process.start(program, arguments);
    return process.waitForStarted(2000) && process.waitForFinished(5000) &&
           process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0;
}

QString archiveRoot(const QString &assetName) {
    QString root = assetName;
    root.chop(root.endsWith(QStringLiteral(".tar.gz")) ? 7 : 4);
    return root;
}

bool writeFile(const QString &path, const QByteArray &contents) {
    QFile file(path);
    return file.open(QIODevice::WriteOnly | QIODevice::NewOnly) &&
           file.write(contents) == contents.size() && file.flush();
}

#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
struct ZipEntry {
    QByteArray name;
    QByteArray contents;
    quint32 attributes = 0100755u << 16;
};

void append16(QByteArray &bytes, quint16 value) {
    bytes.append(static_cast<char>(value & 0xffu));
    bytes.append(static_cast<char>((value >> 8) & 0xffu));
}

void append32(QByteArray &bytes, quint32 value) {
    append16(bytes, static_cast<quint16>(value & 0xffffu));
    append16(bytes, static_cast<quint16>(value >> 16));
}

quint32 crc32(const QByteArray &bytes) {
    quint32 crc = 0xffffffffu;
    for (char byte : bytes) {
        crc ^= static_cast<quint8>(byte);
        for (int bit = 0; bit < 8; ++bit) {
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
        }
    }
    return ~crc;
}

bool writeStoredZip(const QString &path, const QList<ZipEntry> &entries) {
    if (entries.isEmpty() || entries.size() > 0xffff) {
        return false;
    }
    QByteArray archive;
    QByteArray directory;
    for (const ZipEntry &entry : entries) {
        if (entry.name.isEmpty() || entry.name.size() > 0xffff ||
            entry.contents.size() > 1024 * 1024) {
            return false;
        }
        const quint32 offset = static_cast<quint32>(archive.size());
        const quint32 size = static_cast<quint32>(entry.contents.size());
        const quint32 checksum = crc32(entry.contents);
        append32(archive, 0x04034b50u);
        append16(archive, 20);
        append16(archive, 0);
        append16(archive, 0);
        append16(archive, 0);
        append16(archive, 0x21);
        append32(archive, checksum);
        append32(archive, size);
        append32(archive, size);
        append16(archive, static_cast<quint16>(entry.name.size()));
        append16(archive, 0);
        archive.append(entry.name);
        archive.append(entry.contents);

        append32(directory, 0x02014b50u);
        append16(directory, 0x0314);
        append16(directory, 20);
        append16(directory, 0);
        append16(directory, 0);
        append16(directory, 0);
        append16(directory, 0x21);
        append32(directory, checksum);
        append32(directory, size);
        append32(directory, size);
        append16(directory, static_cast<quint16>(entry.name.size()));
        append16(directory, 0);
        append16(directory, 0);
        append16(directory, 0);
        append16(directory, 0);
        append32(directory, entry.attributes);
        append32(directory, offset);
        directory.append(entry.name);
    }
    const quint32 directoryOffset = static_cast<quint32>(archive.size());
    archive.append(directory);
    append32(archive, 0x06054b50u);
    append16(archive, 0);
    append16(archive, 0);
    append16(archive, static_cast<quint16>(entries.size()));
    append16(archive, static_cast<quint16>(entries.size()));
    append32(archive, static_cast<quint32>(directory.size()));
    append32(archive, directoryOffset);
    append16(archive, 0);
    return writeFile(path, archive);
}

QString zipArchiveTool() {
#if defined(Q_OS_WIN)
    return QStandardPaths::findExecutable(
        QStringLiteral("tar.exe"),
        {QDir(qEnvironmentVariable("SystemRoot")).filePath(QStringLiteral("System32"))});
#else
    return QStringLiteral("/usr/bin/tar");
#endif
}

#if defined(Q_OS_WIN)
QList<ZipEntry> runtimeZipEntries(const QString &root, const QString &failure) {
    QList<ZipEntry> entries = {{(root + "/bitfox.exe").toUtf8(), "fixture engine"}};
    entries.append({(root + "/LICENSE").toUtf8(), "fixture license"});
    entries.append({(root + "/THIRD_PARTY_NOTICES.txt").toUtf8(), "fixture notices"});
    QJsonArray dependencies;
    for (const QString &name : {QStringLiteral("msvcp140.dll"),
                               QStringLiteral("vcruntime140.dll"),
                               QStringLiteral("vcruntime140_1.dll")}) {
        const QByteArray data = "fixture runtime " + name.toUtf8();
        const QString path = failure == "bad-path" && name == "msvcp140.dll"
                                 ? "../msvcp140.dll" : name;
        QString hash = QString::fromLatin1(QCryptographicHash::hash(data, QCryptographicHash::Sha256).toHex());
        if (failure == "wrong-hash" && name == "msvcp140.dll") hash = QString(64, '0');
        dependencies.append(QJsonObject{{"path", path}, {"sha256", hash},
                                       {"provider", "msvc"}, {"version", "14.44.35211.0"},
                                       {"copyright", "Microsoft Corporation"}});
        if (failure == "missing-file" && name == "msvcp140.dll") continue;
        ZipEntry entry{(root + "/" + name).toUtf8(), data};
        if (failure == "symlink" && name == "msvcp140.dll") entry.attributes = 0120777u << 16;
        entries.append(entry);
        if (failure == "duplicate" && name == "msvcp140.dll") entries.append(entry);
        if (failure == "duplicate-link" && name == "msvcp140.dll") {
            entries.append({entry.name, "vcruntime140.dll", 0120777u << 16});
        }
    }
    const QByteArray notice("fixture runtime notice");
    entries.append({(root + "/licenses/msvc-2022.txt").toUtf8(), notice});
    const QJsonObject inventory{
        {"schema", 1}, {"platform", "windows"}, {"architecture", "x86_64"},
        {"origin", QJsonObject{{"provider", "Visual Studio 2022"}, {"redist_version", "14.44.35211"},
                              {"directory", "x64/Microsoft.VC143.CRT"}}},
        {"executables", QJsonArray{"bitfox.exe"}},
        {"dependencies", dependencies},
        {"notices", QJsonArray{QJsonObject{
            {"path", "licenses/msvc-2022.txt"}, {"provider", "msvc"}, {"version", "2022"},
            {"source", "https://learn.microsoft.com/visualstudio/releases/2022/redistribution"},
            {"sha256", QString::fromLatin1(QCryptographicHash::hash(notice, QCryptographicHash::Sha256).toHex())}
        }}}
    };
    entries.append({(root + "/RUNTIME_DEPENDENCIES.json").toUtf8(), QJsonDocument(inventory).toJson()});
    return entries;
}
#endif
#endif
}

class ReleaseManagerTest : public QObject {
    Q_OBJECT

private slots:
    void initTestCase();
    void regularEngineIsInstalled();
    void symlinkEngineIsRejected_data();
    void symlinkEngineIsRejected();
    void duplicateEngineIsRejected();
    void invalidDownloadMetadataIsRejected();
    void untrustedDownloadUrlsAreRejected_data();
    void untrustedDownloadUrlsAreRejected();
    void managerDestructionStopsExtractor();
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
    void storedZipFixtureCanBeRead();
#endif
#if defined(Q_OS_WIN)
    void runtimePackageIsInstalledTogether_data();
    void runtimePackageIsInstalledTogether();
    void realRuntimePackageStartsInstalledEngine();
#endif
};

void ReleaseManagerTest::initTestCase() {
    QStandardPaths::setTestModeEnabled(true);
    QCoreApplication::setOrganizationName(QStringLiteral("BitfoxTests"));
    QCoreApplication::setApplicationName(QStringLiteral("ReleaseManagerTest"));
}

void ReleaseManagerTest::regularEngineIsInstalled() {
    auto work = std::make_shared<QTemporaryDir>();
    QVERIFY(work->isValid());
    QTemporaryDir engines;
    QVERIFY(engines.isValid());
    const QString version = QStringLiteral("0.0.0-fixture-") +
                            QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QString tag = QStringLiteral("v") + version;
    const QString asset = QStringLiteral("bitfox-") + version +
                          ReleaseManager::platformAssetSuffix();
    const QString root = archiveRoot(asset);
    const QString archive = work->filePath(asset);
    const QByteArray contents = QByteArrayLiteral("fixture engine\n");
#if defined(Q_OS_WIN)
    const QString engineName = QStringLiteral("bitfox.exe");
    const QString installedName = QStringLiteral("Bitfox ") + version + QStringLiteral(".exe");
    QVERIFY(writeStoredZip(archive, {{(root + "/" + engineName).toUtf8(), contents}}));
#else
    const QString engineName = QStringLiteral("bitfox");
    const QString installedName = QStringLiteral("Bitfox ") + version;
    const QString source = work->filePath(QStringLiteral("source/")) + root;
    QVERIFY(QDir().mkpath(source));
    QVERIFY(writeFile(QDir(source).filePath(engineName), contents));
    QVERIFY(runProcess(QStringLiteral("/usr/bin/tar"),
                       {QStringLiteral("-czf"), archive, QStringLiteral("-C"),
                        work->filePath(QStringLiteral("source")), root}));
#endif
    const QString destination = QDir(engines.path()).filePath(installedName);
    QVERIFY(!QFileInfo::exists(destination));
    const auto cleanup = qScopeGuard([destination] { QFile::remove(destination); });
    ReleaseManager manager(nullptr, engines.path());
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    manager.handleDownloadedArchive(tag, archive, work);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 3000);
    const QList<QVariant> result = finished.takeFirst();
    QCOMPARE(result[0].toString(), tag);
    QVERIFY2(result[1].toBool(), qPrintable(result[2].toString()));
    QVERIFY(result[2].toString().isEmpty());
    const QFileInfo installed(destination);
    QVERIFY(installed.isFile());
    QVERIFY(!installed.isSymLink());
#if !defined(Q_OS_WIN)
    QVERIFY(installed.isExecutable());
#endif
    QFile file(destination);
    QVERIFY(file.open(QIODevice::ReadOnly));
    QCOMPARE(file.readAll(), contents);
    QVERIFY(ReleaseManager::isInstalled(tag, engines.path()));
    file.close();
    QVERIFY(QFile::remove(destination));
    QVERIFY(!ReleaseManager::isInstalled(tag, engines.path()));
}

#if defined(Q_OS_WIN)
void ReleaseManagerTest::runtimePackageIsInstalledTogether_data() {
    QTest::addColumn<QString>("failure");
    for (const char *name : {"none", "wrong-hash", "missing-file", "duplicate", "bad-path",
                             "symlink", "duplicate-link", "existing-directory"}) {
        QTest::newRow(name) << QString::fromLatin1(name);
    }
}

void ReleaseManagerTest::runtimePackageIsInstalledTogether() {
    QFETCH(QString, failure);
    auto work = std::make_shared<QTemporaryDir>();
    QTemporaryDir engines;
    QVERIFY(work->isValid() && engines.isValid());
    const QString asset = "bitfox-2.6.0" + ReleaseManager::platformAssetSuffix();
    const QString root = archiveRoot(asset);
    const QString archive = work->filePath(asset);
    QVERIFY(writeStoredZip(archive, runtimeZipEntries(root, failure)));
    const QString package = QDir(engines.path()).filePath("Bitfox 2.6.0");
    if (failure == "existing-directory") {
        QVERIFY(QDir().mkpath(package));
        QVERIFY(writeFile(QDir(package).filePath("keep.txt"), "user file"));
    }
    ReleaseManager manager(nullptr, engines.path());
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    manager.handleDownloadedArchive("v2.6.0", archive, work);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 15000);
    const auto result = finished.takeFirst();
    QCOMPARE(result[1].toBool(), failure == "none");
    if (failure == "none") {
        QVERIFY2(result[2].toString().isEmpty(), qPrintable(result[2].toString()));
        for (const QString &name : {"Bitfox 2.6.0.exe", "msvcp140.dll", "vcruntime140.dll",
                                    "vcruntime140_1.dll", "licenses/msvc-2022.txt",
                                    "RUNTIME_DEPENDENCIES.json", "LICENSE",
                                    "THIRD_PARTY_NOTICES.txt"}) {
            QVERIFY(QFileInfo(QDir(package).filePath(name)).isFile());
        }
        QVERIFY(ReleaseManager::isInstalled("v2.6.0", engines.path()));
        QFile manifest(QDir(package).filePath("RUNTIME_DEPENDENCIES.json"));
        QVERIFY(manifest.open(QIODevice::ReadOnly));
        QCOMPARE(QJsonDocument::fromJson(manifest.readAll()).object().value("executables").toArray(),
                 QJsonArray{"Bitfox 2.6.0.exe"});
    } else if (failure == "existing-directory") {
        QFile marker(QDir(package).filePath("keep.txt"));
        QVERIFY(marker.open(QIODevice::ReadOnly));
        QCOMPARE(marker.readAll(), QByteArray("user file"));
        QCOMPARE(QDir(package).entryList(QDir::Files), QStringList{"keep.txt"});
    } else {
        QVERIFY(!QFileInfo::exists(package));
        QVERIFY(!ReleaseManager::isInstalled("v2.6.0", engines.path()));
    }
    QCOMPARE(QDir(engines.path()).entryList({"install-*"}, QDir::Dirs | QDir::NoDotAndDotDot).size(), 0);
}

void ReleaseManagerTest::realRuntimePackageStartsInstalledEngine() {
    const QString archive = qEnvironmentVariable("BITFOX_RELEASE_TEST_ARCHIVE");
    if (archive.isEmpty()) QSKIP("A native release archive fixture is required");
    const QString root = archiveRoot(QFileInfo(archive).fileName());
    const QString suffix = "-windows-x86_64";
    QVERIFY(root.startsWith("bitfox-") && root.endsWith(suffix));
    const QString version = root.mid(7, root.size() - 7 - suffix.size());
    QVERIFY(!version.isEmpty());
    auto work = std::make_shared<QTemporaryDir>();
    QTemporaryDir engines;
    QTemporaryDir unrelated;
    QVERIFY(work->isValid() && engines.isValid() && unrelated.isValid());
    ReleaseManager manager(nullptr, engines.path());
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    manager.handleDownloadedArchive("v" + version, archive, work);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 20000);
    const auto result = finished.takeFirst();
    QVERIFY2(result[1].toBool(), qPrintable(result[2].toString()));
    const QString directory = QDir(engines.path()).filePath("Bitfox " + version);
    const QString engine = QDir(directory).filePath("Bitfox " + version + ".exe");
    QVERIFY(QFileInfo::exists(QDir(directory).filePath("vcruntime140.dll")));
    QProcess process;
    QProcessEnvironment environment;
    const QString system = qEnvironmentVariable("SystemRoot");
    environment.insert("SystemRoot", system);
    environment.insert("WINDIR", system);
    environment.insert("PATH", QDir(system).filePath("System32") + ";" + system);
    environment.insert("TEMP", unrelated.path());
    environment.insert("TMP", unrelated.path());
    process.setProcessEnvironment(environment);
    process.setWorkingDirectory(unrelated.path());
    process.start(engine);
    const auto cleanup = qScopeGuard([&process] {
        if (process.state() != QProcess::NotRunning) {
            process.kill();
            process.waitForFinished(5000);
        }
    });
    QVERIFY(process.waitForStarted(5000));
    process.write("uci\nisready\n");
    QByteArray output;
    QElapsedTimer timer;
    timer.start();
    while (!output.contains("readyok") && timer.elapsed() < 5000) {
        process.waitForReadyRead(100);
        output.append(process.readAllStandardOutput());
    }
    QVERIFY2(output.contains("uciok") && output.contains("readyok"), output.constData());
    QVERIFY(output.split('\n').contains(("id name Bitfox " + version).toUtf8() + '\r') ||
            output.split('\n').contains(("id name Bitfox " + version).toUtf8()));
    HANDLE snapshot = INVALID_HANDLE_VALUE;
    for (int attempt = 0; attempt < 3; ++attempt) {
        snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
                                            static_cast<DWORD>(process.processId()));
        if (snapshot != INVALID_HANDLE_VALUE || GetLastError() != ERROR_BAD_LENGTH) break;
        QTest::qWait(20);
    }
    QVERIFY(snapshot != INVALID_HANDLE_VALUE);
    const auto close = qScopeGuard([snapshot] { CloseHandle(snapshot); });
    MODULEENTRY32W entry{};
    entry.dwSize = static_cast<DWORD>(sizeof(entry));
    QVERIFY(Module32FirstW(snapshot, &entry));
    bool runtimeFound = false;
    do {
        const QString name = QString::fromWCharArray(entry.szModule).toLower();
        if (name.startsWith("vcruntime") ||
            (name.startsWith("msvcp") && name.size() > 5 && name.at(5).isDigit()) ||
            name == "concrt140.dll") {
            runtimeFound = runtimeFound || name == "vcruntime140.dll";
            const QString origin = QFileInfo(QString::fromWCharArray(entry.szExePath)).canonicalPath();
            QCOMPARE(origin.toLower(), QDir(directory).canonicalPath().toLower());
        }
    } while (Module32NextW(snapshot, &entry));
    QVERIFY(runtimeFound);
    process.write("position startpos\ngo depth 2\n");
    output.clear();
    timer.restart();
    while (!output.contains("bestmove ") && timer.elapsed() < 5000) {
        process.waitForReadyRead(100);
        output.append(process.readAllStandardOutput());
    }
    QVERIFY2(output.contains("bestmove "), output.constData());
    process.write("quit\n");
    QVERIFY(process.waitForFinished(5000));
    QCOMPARE(process.exitCode(), 0);
}
#endif

void ReleaseManagerTest::symlinkEngineIsRejected_data() {
    QTest::addColumn<bool>("directory");
    QTest::newRow("symlink") << false;
    QTest::newRow("directory") << true;
}

void ReleaseManagerTest::symlinkEngineIsRejected() {
    QFETCH(bool, directory);
    auto work = std::make_shared<QTemporaryDir>();
    QVERIFY(work->isValid());
    const QString asset = QStringLiteral("bitfox-2.5.0") +
                          ReleaseManager::platformAssetSuffix();
    const QString root = archiveRoot(asset);
    const QString archive = work->filePath(asset);
#if defined(Q_OS_WIN)
    const quint32 attributes = directory ? (0040755u << 16) | 0x10u : 0120777u << 16;
    QVERIFY(writeStoredZip(archive,
                           {{(root + QStringLiteral("/bitfox.exe")).toUtf8(),
                             directory ? QByteArray() : QByteArrayLiteral("real-engine"),
                             attributes}}));
#else
    const QString source = work->filePath(QStringLiteral("source/")) + root;
    QVERIFY(QDir().mkpath(source));
    const QString member = QDir(source).filePath(QStringLiteral("bitfox"));
    if (directory) {
        QVERIFY(QDir().mkpath(member));
    } else {
        const QString target = work->filePath(QStringLiteral("source/real-engine"));
        QVERIFY(writeFile(target, QByteArrayLiteral("engine")));
        QVERIFY(QFile::link(target, member));
    }
    QVERIFY(runProcess(QStringLiteral("/usr/bin/tar"),
                       {QStringLiteral("-czf"), archive, QStringLiteral("-C"),
                        work->filePath(QStringLiteral("source")), root}));
#endif

    ReleaseManager manager;
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    manager.handleDownloadedArchive(QStringLiteral("v2.5.0"), archive, work);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 3000);
    const QList<QVariant> result = finished.takeFirst();
    QCOMPARE(result[1].toBool(), false);
    QVERIFY(result[2].toString().contains(QStringLiteral("invalid layout")) ||
            result[2].toString().contains(QStringLiteral("not a regular file")));
}

void ReleaseManagerTest::duplicateEngineIsRejected() {
    auto work = std::make_shared<QTemporaryDir>();
    QVERIFY(work->isValid());
    const QString asset = QStringLiteral("bitfox-2.5.0") +
                          ReleaseManager::platformAssetSuffix();
    const QString root = archiveRoot(asset);
    const QString archive = work->filePath(asset);
#if defined(Q_OS_WIN)
    const QByteArray member = (root + QStringLiteral("/bitfox.exe")).toUtf8();
    QVERIFY(writeStoredZip(archive, {{member, QByteArrayLiteral("first engine")},
                                     {member, QByteArrayLiteral("second engine")}}));
#else
    const QString source = work->filePath(QStringLiteral("source/")) + root;
    QVERIFY(QDir().mkpath(source));
    QVERIFY(writeFile(QDir(source).filePath(QStringLiteral("bitfox")),
                      QByteArrayLiteral("engine")));
    const QString plain = work->filePath(QStringLiteral("archive.tar"));
    const QString sourceRoot = work->filePath(QStringLiteral("source"));
    QVERIFY(runProcess(QStringLiteral("/usr/bin/tar"),
                       {QStringLiteral("-cf"), plain, QStringLiteral("-C"), sourceRoot,
                        root}));
    QVERIFY(runProcess(QStringLiteral("/usr/bin/tar"),
                       {QStringLiteral("-rf"), plain, QStringLiteral("-C"), sourceRoot,
                        root + QStringLiteral("/bitfox")}));
    QVERIFY(runProcess(QStringLiteral("/usr/bin/gzip"),
                       {QStringLiteral("-c"), plain}, archive));
#endif

    const bool wasSet = qEnvironmentVariableIsSet("TAR_OPTIONS");
    const QByteArray previous = qgetenv("TAR_OPTIONS");
    const auto restoreOptions = qScopeGuard([wasSet, previous] {
        if (wasSet) {
            qputenv("TAR_OPTIONS", previous);
        } else {
            qunsetenv("TAR_OPTIONS");
        }
    });
    qputenv("TAR_OPTIONS", "--option-that-must-not-reach-tar");
    ReleaseManager manager;
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    manager.handleDownloadedArchive(QStringLiteral("v2.5.0"), archive, work);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 3000);
    const QList<QVariant> result = finished.takeFirst();
    QCOMPARE(result[1].toBool(), false);
    QVERIFY(result[2].toString().contains(QStringLiteral("invalid layout")));
}

void ReleaseManagerTest::invalidDownloadMetadataIsRejected() {
    ReleaseManager manager;
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    ReleaseInfo info;
    info.tag = QStringLiteral("v2.5.0");
    info.assetName = QStringLiteral("../bitfox.zip");
    info.assetUrl = QStringLiteral("https://example.com/bitfox.zip");
    info.checksumUrl = QStringLiteral("https://example.com/bitfox.zip.sha256");
    manager.downloadRelease(info);
    QCOMPARE(finished.count(), 1);
    QCOMPARE(finished.takeFirst()[1].toBool(), false);
}

void ReleaseManagerTest::untrustedDownloadUrlsAreRejected_data() {
    QTest::addColumn<QString>("prefix");
    QTest::addColumn<bool>("checksum");
    for (bool checksum : {false, true}) {
        const QByteArray suffix = checksum ? "-checksum" : "-archive";
        QTest::newRow(("credentials" + suffix).constData())
            << QStringLiteral("https://user:pass@github.com") << checksum;
        QTest::newRow(("nonstandard-port" + suffix).constData())
            << QStringLiteral("https://github.com:4443") << checksum;
        QTest::newRow(("http" + suffix).constData())
            << QStringLiteral("http://github.com") << checksum;
    }
}

void ReleaseManagerTest::untrustedDownloadUrlsAreRejected() {
    QFETCH(QString, prefix);
    QFETCH(bool, checksum);
    ReleaseManager manager;
    QSignalSpy finished(&manager, &ReleaseManager::downloadFinished);
    ReleaseInfo info;
    info.tag = QStringLiteral("v2.5.0");
    info.assetName = QStringLiteral("bitfox-2.5.0") + ReleaseManager::platformAssetSuffix();
    const QString path = QStringLiteral("/Azeel1/bitfox-engine/releases/download/") +
                         info.tag + '/' + info.assetName;
    info.assetUrl = (checksum ? QStringLiteral("https://github.com") : prefix) + path;
    info.checksumUrl = (checksum ? prefix : QStringLiteral("https://github.com")) +
                       path + QStringLiteral(".sha256");
    manager.downloadRelease(info);
    QCOMPARE(finished.count(), 1);
    QCOMPARE(finished.first()[1].toBool(), false);
    QVERIFY(manager.m_inFlight.isEmpty());
}

void ReleaseManagerTest::managerDestructionStopsExtractor() {
    auto manager = std::make_unique<ReleaseManager>();
    QPointer<QProcess> extractor;
#if defined(Q_OS_WIN)
    extractor = new QProcess(manager.get());
    QSignalSpy started(extractor, &QProcess::started);
    extractor->start(QCoreApplication::applicationFilePath(),
                     {QStringLiteral("--hold-release-process")});
    QVERIFY(extractor->waitForStarted(2000));
    QCOMPARE(started.count(), 1);
#else
    auto work = std::make_shared<QTemporaryDir>();
    QVERIFY(work->isValid());
    const QString archive = work->filePath(QStringLiteral("bitfox-2.5.0") +
                                           ReleaseManager::platformAssetSuffix());
    QVERIFY(::mkfifo(QFile::encodeName(archive).constData(), 0600) == 0);
    manager->handleDownloadedArchive(QStringLiteral("v2.5.0"), archive, work);
    const auto children = manager->findChildren<QProcess *>(QString(), Qt::FindDirectChildrenOnly);
    QCOMPARE(children.size(), 1);
    extractor = children.first();
    QVERIFY(extractor->waitForStarted(2000));
#endif
    QCOMPARE(extractor->state(), QProcess::Running);
    QSignalSpy stopped(extractor, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished));
    QElapsedTimer elapsed;
    elapsed.start();
    manager.reset();
    QVERIFY2(elapsed.elapsed() < 1500, qPrintable(QString::number(elapsed.elapsed())));
    QCOMPARE(stopped.count(), 1);
    QVERIFY(extractor.isNull());
}

#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
void ReleaseManagerTest::storedZipFixtureCanBeRead() {
    QTemporaryDir work;
    QVERIFY(work.isValid());
    const QString archive = work.filePath(QStringLiteral("fixture.zip"));
    const QByteArray contents = QByteArrayLiteral("fixture engine\n");
    QVERIFY(writeStoredZip(archive, {{QByteArrayLiteral("root/bitfox.exe"), contents},
                                     {QByteArrayLiteral("root/duplicate"), QByteArrayLiteral("first")},
                                     {QByteArrayLiteral("root/duplicate"), QByteArrayLiteral("second")},
                                     {QByteArrayLiteral("root/symlink"), QByteArrayLiteral("bitfox.exe"),
                                      0120777u << 16},
                                     {QByteArrayLiteral("root/directory"), QByteArray(),
                                      (0040755u << 16) | 0x10u}}));
    const QString tool = zipArchiveTool();
    QVERIFY(!tool.isEmpty());
    const QString listingPath = work.filePath(QStringLiteral("listing"));
    QVERIFY(runProcess(tool, {QStringLiteral("-tvf"), archive}, listingPath));
    QFile listingFile(listingPath);
    QVERIFY(listingFile.open(QIODevice::ReadOnly));
    const QByteArray listing = listingFile.readAll();
    int duplicates = 0;
    bool symlink = false;
    bool directory = false;
    for (QByteArray line : listing.split('\n')) {
        if (line.endsWith('\r')) {
            line.chop(1);
        }
        duplicates += line.endsWith(QByteArrayLiteral(" root/duplicate"));
        symlink = symlink || (line.startsWith('l') && line.contains("root/symlink"));
        directory = directory || (line.startsWith('d') && line.contains("root/directory"));
    }
    QCOMPARE(duplicates, 2);
    QVERIFY(symlink);
    QVERIFY(directory);
    const QString extractedPath = work.filePath(QStringLiteral("extracted"));
    QVERIFY(runProcess(tool, {QStringLiteral("-xOf"), archive, QStringLiteral("root/bitfox.exe")},
                       extractedPath));
    QFile extracted(extractedPath);
    QVERIFY(extracted.open(QIODevice::ReadOnly));
    QCOMPARE(extracted.readAll(), contents);
}
#endif

int main(int argc, char *argv[]) {
    QCoreApplication application(argc, argv);
    if (application.arguments().contains(QStringLiteral("--hold-release-process"))) {
        QEventLoop loop;
        return loop.exec();
    }
    ReleaseManagerTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "ReleaseManagerTest.moc"

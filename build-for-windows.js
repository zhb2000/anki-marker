/** Build portable zip and bundle installer for Windows. */

import { spawn } from 'child_process';
import fs from 'fs';
import path from 'path';
import archiver from 'archiver';
import { fileURLToPath } from 'url';
import { dirname } from 'path';
// ES模块中处理__dirname
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/**
 * @param {string} productName
 * @param {string} portableName
 */
function makePortable(productName, portableName) {
    const releaseDir = path.join(__dirname, 'src-tauri/target/release');
    const portableDir = path.join(releaseDir, 'portable');
    const packDir = path.join(portableDir, portableName);

    // 在 release 目录下创建 portable 目录
    if (!fs.existsSync(packDir)) {
        fs.mkdirSync(packDir, { recursive: true });
    }
    // 拷贝 exe 和 resources 目录到 portable 目录
    fs.copyFileSync(
        path.join(releaseDir, `${productName}.exe`),
        path.join(packDir, `${productName}.exe`)
    );
    fs.cpSync(
        path.join(releaseDir, 'resources'),
        path.join(packDir, 'resources'),
        { recursive: true }
    );
    fs.unlinkSync(path.join(packDir, 'resources/icon.ico'));
    // 拷贝并重命名 config-template.toml 为 config.toml
    fs.copyFileSync(
        path.join(releaseDir, 'resources/config-template.toml'),
        path.join(packDir, 'config.toml')
    );

    // 将压缩 portable 目录为 zip
    console.log(`开始压缩 ${portableName}.zip ...`);
    const output = fs.createWriteStream(path.join(portableDir, `${portableName}.zip`));
    output.on('close', function () {
        const sizeInBytes = archive.pointer();
        const sizeInMegabytes = (sizeInBytes / (1024 * 1024)).toFixed(2);
        console.log(`${portableName}.zip 压缩完成，总大小: ${sizeInMegabytes} MB`);
    });
    const archive = archiver('zip', {
        zlib: { level: 9 } // 设置压缩级别
    });
    archive.on('error', function (err) {
        throw err;
    });
    archive.pipe(output);
    archive.directory(packDir, false);
    archive.finalize();
}

function main() {
    /** @type {boolean} */
    const skipBuild = process.argv.includes('--skip-build');
    const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, 'package.json'), 'utf8'));
    /** @type {string} */
    const version = packageJson.version;
    const tauriJson = JSON.parse(fs.readFileSync(path.join(__dirname, 'src-tauri/tauri.conf.json'), 'utf8'));
    /** @type {string} */
    const productName = tauriJson.package.productName;
    const portableName = `${productName}_${version}_windows-portable`;
    if (skipBuild) {
        console.log('tauri build 已跳过');
        makePortable(productName, portableName);
        return;
    }
    // 使用 spawn 替代 exec 来实时显示输出
    // 使用 stdio: 'inherit' 保证 tauri 的彩色输出
    const buildProcess = spawn('npm', ['run', 'tauri', 'build'], { shell: true, stdio: 'inherit' });
    buildProcess.on('close', code => {
        if (code !== 0) {
            console.error(`tauri build 失败，退出码 ${code}`);
            return;
        }
        makePortable(productName, portableName);
    });
}

main();

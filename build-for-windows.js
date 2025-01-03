#!/usr/bin/env node
/** Build portable zip and bundle installer for Windows. */

import { spawn } from 'child_process';
import fs from 'fs';
import fsp from 'fs/promises'; // 使用 fs/promises 提供的异步方法
import path from 'path';
import archiver from 'archiver';
import { fileURLToPath } from 'url';
import { dirname } from 'path';
// ES 模块中处理 __dirname
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/**
 * @param {string} productName
 * @param {string} packageName
 * @param {string} version
 */
async function renameAndCopyAssets(productName, packageName, version) {
    const releaseDir = path.join(__dirname, 'src-tauri/target/release');
    const assetsDir = path.join(releaseDir, 'release-assets', version);

    // 创建 release-assets 目录
    await fsp.mkdir(assetsDir, { recursive: true });

    // 复制 portable zip
    const portableFilename = `${packageName}_${version}_windows_x64-portable.zip`;
    const portableOldPath = path.join(releaseDir, 'portable', portableFilename);
    const portableNewPath = path.join(assetsDir, portableFilename);
    await fsp.copyFile(portableOldPath, portableNewPath);

    // 复制 msi 文件
    const msiOldFilename = `${productName}_${version}_x64_zh-CN.msi`;
    const msiOldPath = path.join(releaseDir, 'bundle/msi', msiOldFilename);
    const msiNewFilename = `${packageName}_${version}_windows_x64.msi`;
    const msiNewPath = path.join(assetsDir, msiNewFilename);
    await fsp.copyFile(msiOldPath, msiNewPath);

    // 复制 nsis 文件
    const nsisOldFilename = `${productName}_${version}_x64-setup.exe`;
    const nsisOldPath = path.join(releaseDir, 'bundle/nsis', nsisOldFilename);
    const nsisNewFilename = `${packageName}_${version}_windows_x64-setup.exe`;
    const nsisNewPath = path.join(assetsDir, nsisNewFilename);
    await fsp.copyFile(nsisOldPath, nsisNewPath);
}

/**
 * @param {string} productName
 * @param {string} portableName
 */
async function makePortable(productName, portableName) {
    const releaseDir = path.join(__dirname, 'src-tauri/target/release');
    const portableDir = path.join(releaseDir, 'portable');
    const packDir = path.join(portableDir, portableName);

    // 创建 portable 目录
    await fsp.mkdir(packDir, { recursive: true });

    // 拷贝 exe 和 resources 目录到 portable 目录
    await fsp.copyFile(
        path.join(releaseDir, `${productName}.exe`),
        path.join(packDir, `${productName}.exe`)
    );
    await fsp.cp(
        path.join(releaseDir, 'resources'),
        path.join(packDir, 'resources'),
        { recursive: true }
    );

    // 删除 resources/icon.ico
    await fsp.unlink(path.join(packDir, 'resources/icon.ico'));

    // 拷贝并重命名 config-template.toml 为 config.toml
    await fsp.copyFile(
        path.join(releaseDir, 'resources/config-template.toml'),
        path.join(packDir, 'config.toml')
    );

    // 压缩 portable 目录为 zip
    console.log(`开始压缩 ${portableName}.zip ...`);
    await new Promise((resolve, reject) => {
        const output = fs.createWriteStream(path.join(portableDir, `${portableName}.zip`));
        const archive = archiver('zip', { zlib: { level: 9 } });

        output.on('close', () => {
            const sizeInBytes = archive.pointer();
            const sizeInMegabytes = (sizeInBytes / (1024 * 1024)).toFixed(2);
            console.log(`${portableName}.zip 压缩完成，总大小: ${sizeInMegabytes} MB`);
            resolve();
        });

        archive.on('error', err => reject(err));

        archive.pipe(output);
        archive.directory(packDir, false);
        archive.finalize();
    });
}

/**
 * 执行命令行命令的封装
 * @param {string} command
 * @param {string[]} args
 */
async function runCommand(command, args) {
    return new Promise((resolve, reject) => {
        // 使用 spawn 替代 exec 来实时显示输出
        // 使用 stdio: 'inherit' 保证 tauri 的彩色输出
        const process = spawn(command, args, { shell: true, stdio: 'inherit' });
        process.on('close', code => {
            if (code !== 0) {
                reject(new Error(`Command "${command} ${args.join(' ')}" exited with code ${code}`));
            } else {
                resolve();
            }
        });
    });
}

async function main() {
    const skipBuild = process.argv.includes('--skip-build');
    const packageJson = JSON.parse(await fsp.readFile(path.join(__dirname, 'package.json'), 'utf8'));
    /** @type {string} */
    const version = packageJson.version;
    /** @type {string} */
    const packageName = packageJson.name;
    const tauriJson = JSON.parse(await fsp.readFile(path.join(__dirname, 'src-tauri/tauri.conf.json'), 'utf8'));
    /** @type {string} */
    const productName = tauriJson.productName;
    const portableName = `${packageName}_${version}_windows_x64-portable`;

    if (!skipBuild) {
        console.log('开始构建 Tauri 项目...');
        await runCommand('npm', ['run', 'tauri', 'build']);
    } else {
        console.log('跳过 npm run tauri build');
    }
    await makePortable(productName, portableName);
    await renameAndCopyAssets(productName, packageName, version);
    console.log(`构建和打包完成！请查看 src-tauri/target/release/release-assets/${version} 目录。`);
}

main();

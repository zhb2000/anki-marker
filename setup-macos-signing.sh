#!/usr/bin/env bash
#
# 在本机钥匙串中创建 macOS 构建所需的自签名代码签名证书（幂等，可重复执行）。
#
# 背景：macOS 的隐私权限（如辅助功能）与代码签名的 designated requirement 绑定。
# 不签名（ad-hoc）的构建在每次重新编译后权限都会失效，因此需要一个固定的签名
# 身份。本脚本生成自签名证书，不依赖 Apple 开发者账号与完整版 Xcode。
# src-tauri/tauri.macos.conf.json 中的 signingIdentity 依赖此证书。
#
# 注意：证书私钥只存在于本机钥匙串中，请勿将私钥或 .p12 文件提交到仓库。

set -euo pipefail

CERT_NAME="Anki Marker Dev"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CERT_NAME\""; then
  echo "证书 \"$CERT_NAME\" 已存在，跳过创建。"
  exit 0
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

cat > "$workdir/openssl.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions    = v3_code_signing
prompt             = no

[dn]
CN = $CERT_NAME

[v3_code_signing]
basicConstraints = critical, CA:false
keyUsage         = critical, digitalSignature
extendedKeyUsage = codeSigning
EOF

echo "生成自签名代码签名证书 \"$CERT_NAME\"（有效期 10 年）..."
openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -config "$workdir/openssl.cnf" \
  -keyout "$workdir/key.pem" \
  -out "$workdir/cert.pem"

# -keypbe/-certpbe/-macalg 强制传统算法：新版 LibreSSL/OpenSSL 默认的
# PBES2 + HMAC-SHA256 参数 macOS security import 可能不支持。
# 密码不能为空：RFC 7292 中“空字符串密码”与“NULL 密码”是两种编码，
# security import 处理空密码 PKCS#12 时会报 "MAC verification failed"。
P12_PASS="$(openssl rand -hex 16)"

openssl pkcs12 -export \
  -inkey "$workdir/key.pem" -in "$workdir/cert.pem" \
  -out "$workdir/cert.p12" -passout "pass:$P12_PASS" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1

echo "导入登录钥匙串（之后首次签名若弹出钥匙串授权框，请点“始终允许”）..."
if ! security import "$workdir/cert.p12" -k "$KEYCHAIN" -P "$P12_PASS" -T /usr/bin/codesign; then
  echo "PKCS#12 导入失败，回退为分别导入 PEM 私钥与证书..."
  security import "$workdir/key.pem" -k "$KEYCHAIN" -T /usr/bin/codesign
  security import "$workdir/cert.pem" -k "$KEYCHAIN" -T /usr/bin/codesign
fi

# 信任设置为可选：签名与 TCC 权限校验并不依赖它，仅影响 codesign --verify 等严格校验
echo "将证书设为受信任..."
security add-trusted-cert -r trustRoot -k "$KEYCHAIN" "$workdir/cert.pem" \
  || echo "信任设置未完成（不影响构建与权限）。如需要，可在“钥匙串访问”中手动将该证书设为始终信任。"

echo "完成。当前可用的代码签名身份："
security find-identity -v -p codesigning | grep "\"$CERT_NAME\""

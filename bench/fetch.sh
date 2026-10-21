#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

LYNC_COMMIT=3dbcbd3
QUICKNET_COMMIT=edba9a2
BLINK_VERSION=v0.18.9
BLINKBLOX_VERSION=v1.3.1
ZAP_VERSION=v0.6.29
NETRAY_VERSION=v0.1.1-cli.1
SQUASH_VERSION=v5.0.0
SATSET_VERSION=v0.5.0
BYTENET_COMMIT=92addc1
BITNET_COMMIT=f16394b
PAKNET_VERSION=v0.9.0
PACK_VERSION=v0.12.1
WORK=$(mktemp -d)

Clone() {
	git clone --quiet "https://github.com/$1.git" "$WORK/$2"
	git -C "$WORK/$2" checkout --quiet "$3"
}

Clone Axp3cter/Lync lync "$LYNC_COMMIT"
rm -rf vendor/lync
cp -r "$WORK/lync/src" vendor/lync
cp "$WORK/lync/LICENSE" vendor/lync.LICENSE

Clone breadboardengineer1234/QuickNet quicknet "$QUICKNET_COMMIT"
rm -rf vendor/quicknet vendor/quicknet-raised
cp -r "$WORK/quicknet/src" vendor/quicknet
cp "$WORK/quicknet/LICENSE" vendor/quicknet.LICENSE
cp -r vendor/quicknet vendor/quicknet-raised
sed -i -e 's/MaxCallsPerFrame = 100,/MaxCallsPerFrame = 65535,/' \
	-e 's/MaxPacketSize = 50000,/MaxPacketSize = 2000000,/' vendor/quicknet-raised/Config.luau

Clone Data-Oriented-House/Squash squash "$SQUASH_VERSION"
rm -rf vendor/squash
mkdir -p vendor/squash
cp "$WORK/squash/src/init.luau" vendor/squash/init.luau
cp "$WORK/squash/LICENSE" vendor/squash.LICENSE

Clone bookek/satset satset "$SATSET_VERSION"
rm -rf vendor/satset
cp -r "$WORK/satset/src" vendor/satset
cp "$WORK/satset/LICENSE" vendor/satset.LICENSE

Clone ffrostfall/ByteNet bytenet "$BYTENET_COMMIT"
rm -rf vendor/bytenet
cp -r "$WORK/bytenet/src" vendor/bytenet
rm vendor/bytenet/index.d.ts vendor/bytenet/dataTypes/README.md
cp "$WORK/bytenet/LICENSE" vendor/bytenet.LICENSE

Clone minecoiii2/bitnet bitnet "$BITNET_COMMIT"
rm -rf vendor/bitnet
cp -r "$WORK/bitnet/BitNet" vendor/bitnet
cp "$WORK/bitnet/LICENSE" vendor/bitnet.LICENSE

Clone isoopod/PakNet paknet "$PAKNET_VERSION"
Clone isoopod/Pack pack "$PACK_VERSION"
for Package in data-oriented-house/lemonsignal/1.11.0 howmanysmall/typed-promise/4.0.6 evaera/promise/4.0.0; do
	Name=$(basename "$(dirname "$Package")")
	curl -sSfL -H "Wally-Version: 0.3.2" "https://api.wally.run/v1/package-contents/$Package" -o "$WORK/$Name.zip"
	unzip -q "$WORK/$Name.zip" -d "$WORK/$Name"
done
rm -rf vendor/paknet vendor/paknet*.LICENSE
mkdir -p vendor/paknet/roblox_packages/_Index/typed-promise
cp -r "$WORK/paknet/src" vendor/paknet/PakNet
cp -r "$WORK/pack/src" vendor/paknet/roblox_packages/Pack
cp "$WORK/lemonsignal/src/LemonSignal.lua" vendor/paknet/roblox_packages/Signal.lua
cp -r "$WORK/typed-promise/src" vendor/paknet/roblox_packages/_Index/typed-promise/typed-promise
cp "$WORK/promise/lib/init.lua" vendor/paknet/roblox_packages/_Index/typed-promise/Promise.lua
printf '%s\n' 'return require(script.PakNet)' > vendor/paknet/init.luau
printf '%s\n' 'return require(script.Parent._Index["typed-promise"]["typed-promise"])' > vendor/paknet/roblox_packages/Promise.luau
cp "$WORK/paknet/LICENSE" vendor/paknet.LICENSE
cp "$WORK/pack/LICENSE" vendor/paknet-pack.LICENSE
cp "$WORK/lemonsignal/LICENSE" vendor/paknet-lemonsignal.LICENSE
cp "$WORK/typed-promise/LICENSE" vendor/paknet-typed-promise.LICENSE
cp "$WORK/promise/LICENSE" vendor/paknet-promise.LICENSE

rm -rf vendor/packet-raised
if [ -d vendor/packet ]; then
	cp -r vendor/packet vendor/packet-raised
	sed -i 's/if bytes > 8_000 then/if bytes > 2_000_000_000 then/' vendor/packet-raised/init.luau
else
	echo "vendor/packet is missing, so Packet and PacketRaised will not load; see the note at the top"
fi

gh release download "$BLINK_VERSION" --repo 1Axen/blink -p "blink-windows-x86_64.tar.gz" -D "$WORK"
tar xzf "$WORK/blink-windows-x86_64.tar.gz" -C "$WORK"
"$WORK/blink.exe" Bench.blink -y -q

gh release download "$BLINKBLOX_VERSION" --repo XopoIII/BlinkBlox -p "blinkblox-windows-x86_64.tar.gz" -D "$WORK"
tar xzf "$WORK/blinkblox-windows-x86_64.tar.gz" -C "$WORK"
rm -rf vendor/blinkblox
"$WORK/blinkblox.exe" Bench.blinkblox. -y -q
curl -fsSL "https://raw.githubusercontent.com/XopoIII/BlinkBlox/$BLINKBLOX_VERSION/LICENSE" -o vendor/blinkblox.LICENSE

gh release download "$ZAP_VERSION" --repo red-blox/zap -p "zap-${ZAP_VERSION#v}-windows-x86_64.zip" -D "$WORK"
unzip -q "$WORK/zap-${ZAP_VERSION#v}-windows-x86_64.zip" -d "$WORK"
rm -rf vendor/zap
"$WORK/zap.exe" Bench.zap
gh api "repos/red-blox/zap/contents/LICENSE?ref=$ZAP_VERSION" -H "Accept: application/vnd.github.raw" > vendor/zap.LICENSE

git clone --quiet --depth 1 --branch "$NETRAY_VERSION" https://github.com/AstaWasTaken/NetRay-Compile.git "$WORK/netray"
gh release download "$NETRAY_VERSION" --repo AstaWasTaken/NetRay-Compile -p "NetRay-Compile-windows-x86_64.zip" -D "$WORK"
unzip -q "$WORK/NetRay-Compile-windows-x86_64.zip" -d "$WORK"
rm -rf vendor/netraycompile
"$WORK/NetRay-Compile.exe" compile Bench.idl --out-dir vendor/netraycompile --scope NetRayCompile
cp "$WORK/netray/LICENSE" vendor/netraycompile.LICENSE

../compiler/target/release/courier.exe --runtime 'game:GetService("ReplicatedStorage"):WaitForChild("Courier")' Bench.courier vendor/net
rm -rf "$WORK"

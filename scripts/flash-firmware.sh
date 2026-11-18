#!/usr/bin/env bash
# Write a firmware image to the macro pad's CH552 chip.
#
#   scripts/flash-firmware.sh <firmware.hex|.bin> [seconds to wait, default 300]
#
# Waits for the pad to appear in the chip's flashing mode (USB 4348:55e0), then flashes
# and verifies the image with wchisp. The bootloader leaves flashing mode on its own
# after about 10 seconds, so this starts the moment the pad shows up. To enter
# flashing mode, short the two SW2 pads on the board while plugging the pad in.
set -euo pipefail

image="${1:?usage: flash-firmware.sh <firmware.hex|.bin> [seconds]}"
wait_s="${2:-300}"
wchisp="${WCHISP:-$(command -v wchisp || echo "$HOME/.cargo/bin/wchisp")}"
[ -f "$image" ] || { echo "no such file: $image" >&2; exit 1; }
[ -x "$wchisp" ] || { echo "wchisp not found (cargo install wchisp)" >&2; exit 1; }

say_() { command -v say >/dev/null && say "$1" & }

echo "waiting up to ${wait_s}s for the pad in flashing mode (short SW2 while plugging in)..."
end=$(( $(date +%s) + wait_s ))
until ioreg -p IOUSB -l -w0 | grep -qE '"idVendor" = 17224'; do
  if [ "$(date +%s)" -ge "$end" ]; then
    echo "timed out: the pad never entered flashing mode" >&2
    exit 1
  fi
  sleep 0.2
done

echo "flashing mode found, writing $image"
say_ "found, flashing"
if "$wchisp" flash "$image"; then
  echo "done"
  say_ "flashing done"
else
  say_ "flashing failed"
  exit 1
fi

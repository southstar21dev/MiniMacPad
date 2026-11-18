# MiniMacPad firmware for the CH552G six-key macro pad.
#
#   make              build build/minimacpad.hex, the MiniMacPad firmware
#   make release      build it and copy it to minimacpad.hex, the copy the app carries
#                     (also bump USB_DEVICE_VERSION in src/config.h and FIRMWARE_VERSION
#                     in ../src/pad.rs)
#   make flash        build it, wait for the pad in flashing mode and write it
#   make probe        build probe.hex, the pin-finding test firmware
#   make flash-probe  build and write the probe
#   make clean
#
# Needs SDCC (brew install sdcc) and wchisp (cargo install wchisp). To put the pad in
# flashing mode, hold the knob down while plugging it in (MiniMacPad firmware), or
# short the two SW2 pads on the board while plugging it in (any firmware).

FREQ_SYS  = 16000000
XRAM_LOC  = 0x0100
XRAM_SIZE = 0x0300
CODE_SIZE = 0x3800

CC      = sdcc
WCHISP ?= wchisp
CFLAGS  = -mmcs51 --model-small --no-xinit-opt
CFLAGS += --xram-size $(XRAM_SIZE) --xram-loc $(XRAM_LOC) --code-size $(CODE_SIZE)
CFLAGS += -Isrc -DF_CPU=$(FREQ_SYS)

BUILD = build
LIB   = src/delay.c src/usb_descr.c src/usb_handler.c src/usb_hid.c

.PHONY: all release flash probe flash-probe clean

all: $(BUILD)/minimacpad.hex

release: $(BUILD)/minimacpad.hex
	cp $< minimacpad.hex

probe: $(BUILD)/probe.hex

$(BUILD)/%.rel: %.c $(wildcard src/*.h)
	@mkdir -p $(dir $@)
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD)/probe.ihx: $(addprefix $(BUILD)/,$(LIB:.c=.rel)) $(BUILD)/probe.rel
	$(CC) $(CFLAGS) $(BUILD)/probe.rel $(addprefix $(BUILD)/,$(LIB:.c=.rel)) -o $@

$(BUILD)/minimacpad.ihx: $(addprefix $(BUILD)/,$(LIB:.c=.rel)) $(BUILD)/src/neo.rel $(BUILD)/minimacpad.rel
	$(CC) $(CFLAGS) $(BUILD)/minimacpad.rel $(addprefix $(BUILD)/,$(LIB:.c=.rel)) $(BUILD)/src/neo.rel -o $@

$(BUILD)/%.hex: $(BUILD)/%.ihx
	packihx $< > $@

flash: $(BUILD)/minimacpad.hex
	../scripts/flash-firmware.sh $<

flash-probe: $(BUILD)/probe.hex
	../scripts/flash-firmware.sh $<

clean:
	rm -rf $(BUILD)

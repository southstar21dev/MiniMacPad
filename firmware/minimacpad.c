// ===================================================================================
// MiniMacPad firmware for the CH552G six-key, one-knob macro pad (USB 1189:8890)
// ===================================================================================
//
// The keys and the knob send the key codes the MiniMacPad background service listens
// for (0xF0..0xF8, unassigned in the USB keyboard standard, so nothing else reacts),
// which makes the old "program the pad" step unnecessary.
//
// The key backlight is set by the MiniMacPad app with vendor output report 5:
//
//   05 01 effect r g b save 00   set the backlight; save=1 also keeps it after unplugging
//   05 02 'B' 'O' 'O' 'T' 00 00  restart into the chip's USB flashing mode
//
// Effects: 0 off, 1 solid colour, 2 lit while pressed, 3 rainbow (brightness taken
// from the colour). The firmware version is the USB device version (bcdDevice).
//
// To flash new firmware without the app, hold the knob down while plugging the pad in:
// the keys turn blue and the chip waits in its USB flashing mode for about 10 seconds.
//
// Based on the CH552 MacroPad plus by Stefan Wagner (CC BY-SA 3.0).

#include "src/config.h"
#include "src/system.h"
#include "src/gpio.h"
#include "src/delay.h"
#include "src/neo.h"
#include "src/usb_hid.h"

// USB interrupt, handled by the library.
void USB_interrupt(void);
void USB_ISR(void) __interrupt(INT_NO_USB) {
  USB_interrupt();
}

// Key codes, in the order of MiniMacPad's controls (src/device.rs CONTROLS).
#define CODE_KEY1        0xF0
#define CODE_KNOB_LEFT   0xF6
#define CODE_KNOB_PRESS  0xF7
#define CODE_KNOB_RIGHT  0xF8

#define BUTTONS          7                  // six keys and the knob press
#define DEBOUNCE_MS      5                  // a change must hold this long to count

#define EFFECT_OFF       0
#define EFFECT_SOLID     1
#define EFFECT_PRESS     2
#define EFFECT_RAINBOW   3
#define EFFECTS          4

#define CMD_BACKLIGHT    1
#define CMD_BOOT         2

// Settings kept in the chip's data flash: a marker byte, then effect, r, g, b.
#define SETTINGS_MARK    0x5A
#define LED_SUM_MAX      300                // cap on r+g+b per LED, to stay within USB power

// Which LED in the chain sits under each key (keys numbered left to right, top to
// bottom). The chain runs up the left column, then up the right one.
__code uint8_t KEY_LED[6] = {2, 5, 1, 4, 0, 3};

// Keyboard report: id 1, modifiers, reserved, up to five key codes.
__xdata uint8_t report[8] = {1, 0, 0, 0, 0, 0, 0, 0};

uint8_t effect = EFFECT_SOLID;
uint8_t red = 96, green = 96, blue = 96;    // solid white until the app sets a colour
uint8_t hue = 0;                            // rainbow position, 0..191
__bit leds_dirty = 1;                       // redraw the LEDs on the next pass
__idata uint8_t held[BUTTONS];              // debounced state per button

// -----------------------------------------------------------------------------------
// Data flash (128 bytes at 0xC000, byte-wide, word-addressed)
// -----------------------------------------------------------------------------------

static uint8_t eeprom_read(uint8_t addr) {
  ROM_ADDR_H = DATA_FLASH_ADDR >> 8;
  ROM_ADDR_L = addr << 1;
  ROM_CTRL = ROM_CMD_READ;
  return ROM_DATA_L;
}

static void eeprom_write(uint8_t addr, uint8_t value) {
  if(eeprom_read(addr) == value) return;    // spare the flash
  SAFE_MOD = 0x55;
  SAFE_MOD = 0xAA;
  GLOBAL_CFG |= bDATA_WE;
  SAFE_MOD = 0;
  ROM_ADDR_H = DATA_FLASH_ADDR >> 8;
  ROM_ADDR_L = addr << 1;
  ROM_DATA_L = value;
  if(ROM_STATUS & bROM_ADDR_OK) ROM_CTRL = ROM_CMD_WRITE;
  SAFE_MOD = 0x55;
  SAFE_MOD = 0xAA;
  GLOBAL_CFG &= ~bDATA_WE;
  SAFE_MOD = 0;
}

static void settings_load(void) {
  if(eeprom_read(0) != SETTINGS_MARK) return;
  effect = eeprom_read(1);
  if(effect >= EFFECTS) effect = EFFECT_SOLID;
  red = eeprom_read(2);
  green = eeprom_read(3);
  blue = eeprom_read(4);
}

static void settings_save(void) {
  eeprom_write(1, effect);
  eeprom_write(2, red);
  eeprom_write(3, green);
  eeprom_write(4, blue);
  eeprom_write(0, SETTINGS_MARK);
}

// -----------------------------------------------------------------------------------
// LEDs
// -----------------------------------------------------------------------------------

// Scale a colour down so its channels add up to at most LED_SUM_MAX.
static void write_led(uint8_t led, uint8_t r, uint8_t g, uint8_t b) {
  uint16_t sum = (uint16_t)r + g + b;
  if(sum > LED_SUM_MAX) {                   // 32-bit: 255 * 300 does not fit 16 bits
    r = (uint32_t)r * LED_SUM_MAX / sum;
    g = (uint32_t)g * LED_SUM_MAX / sum;
    b = (uint32_t)b * LED_SUM_MAX / sum;
  }
  NEO_writeColor(led, r, g, b);
}

static void leds_fill(uint8_t r, uint8_t g, uint8_t b) {
  uint8_t i;
  for(i = 0; i < NEO_COUNT; i++) write_led(i, r, g, b);
  NEO_update();
}

// Colour on the hue circle (0..191) at the given peak level.
static void write_hue(uint8_t led, uint8_t h, uint8_t level) {
  uint8_t up = (uint16_t)(h & 63) * level / 63;
  uint8_t down = level - up;
  switch(h >> 6) {
    case 0:  write_led(led, down, up, 0); break;
    case 1:  write_led(led, 0, down, up); break;
    default: write_led(led, up, 0, down); break;
  }
}

static void leds_draw(void) {
  uint8_t i, level;
  switch(effect) {
    case EFFECT_SOLID:
      leds_fill(red, green, blue);
      return;
    case EFFECT_PRESS:
      for(i = 0; i < 6; i++) {
        if(held[i]) write_led(KEY_LED[i], red, green, blue);
        else write_led(KEY_LED[i], 0, 0, 0);
      }
      break;
    case EFFECT_RAINBOW:
      level = red > green ? red : green;
      if(blue > level) level = blue;
      for(i = 0; i < 6; i++) write_hue(KEY_LED[i], (hue + i * 32) % 192, level);
      break;
    default:
      leds_fill(0, 0, 0);
      return;
  }
  NEO_update();
}

// -----------------------------------------------------------------------------------
// Keys and knob
// -----------------------------------------------------------------------------------

// True while button i (0..5 keys, 6 knob press) is held down.
static __bit button_down(uint8_t i) {
  switch(i) {
    case 0:  return !PIN_read(PIN_KEY1);
    case 1:  return !PIN_read(PIN_KEY2);
    case 2:  return !PIN_read(PIN_KEY3);
    case 3:  return !PIN_read(PIN_KEY4);
    case 4:  return !PIN_read(PIN_KEY5);
    case 5:  return !PIN_read(PIN_KEY6);
    default: return !PIN_read(PIN_ENC_SW);
  }
}

static uint8_t button_code(uint8_t i) {
  return i < 6 ? CODE_KEY1 + i : CODE_KNOB_PRESS;
}

// Add a held key code to the report, or remove it, and send the report.
static void set_code(uint8_t code, __bit down) {
  uint8_t i;
  for(i = 3; i < 8; i++) {
    if(down ? report[i] == 0 : report[i] == code) {
      report[i] = down ? code : 0;
      HID_sendReport(report, sizeof(report));
      return;
    }
  }
}

// One knob step: the code goes down and straight back up.
static void tap(uint8_t code) {
  set_code(code, 1);
  set_code(code, 0);
}

// Knob contacts as a 2-bit state: bit 1 is A, bit 0 is B, 1 = open (high).
static uint8_t knob_state(void) {
  return (PIN_read(PIN_ENC_A) ? 2 : 0) | (PIN_read(PIN_ENC_B) ? 1 : 0);
}

// Quadrature steps, indexed by old state * 4 + new state: +1 is a quarter step to the
// right (A closes first), -1 to the left, 0 no movement or an impossible jump.
__code int8_t KNOB_STEP[16] = {0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1, -1, 0};

// -----------------------------------------------------------------------------------
// Commands from the app
// -----------------------------------------------------------------------------------

static void handle_command(void) {
  if(HID_cmd[1] == CMD_BACKLIGHT && HID_cmd[2] < EFFECTS) {
    effect = HID_cmd[2];
    red = HID_cmd[3];
    green = HID_cmd[4];
    blue = HID_cmd[5];
    if(HID_cmd[6]) settings_save();
    leds_dirty = 1;
  } else if(HID_cmd[1] == CMD_BOOT && HID_cmd[2] == 'B' && HID_cmd[3] == 'O'
            && HID_cmd[4] == 'O' && HID_cmd[5] == 'T') {
    leds_fill(0, 0, 96);
    DLY_ms(50);
    BOOT_now();
  }
  HID_cmdReady = 0;
}

void main(void) {
  __idata uint8_t settle[BUTTONS];          // ms a differing reading has lasted
  uint8_t i, knob, now, tick = 0;
  int8_t turn = 0;                          // quarter steps since the knob's rest state

  NEO_init();
  CLK_config();
  DLY_ms(10);

  PIN_input_PU(PIN_KEY1);
  PIN_input_PU(PIN_KEY2);
  PIN_input_PU(PIN_KEY3);
  PIN_input_PU(PIN_KEY4);
  PIN_input_PU(PIN_KEY5);
  PIN_input_PU(PIN_KEY6);
  PIN_input_PU(PIN_ENC_A);
  PIN_input_PU(PIN_ENC_B);
  PIN_input_PU(PIN_ENC_SW);
  DLY_ms(5);

  // Knob held while plugging in: wait in the chip's flashing mode instead.
  if(!PIN_read(PIN_ENC_SW)) {
    leds_fill(0, 0, 96);
    BOOT_now();
  }

  for(i = 0; i < BUTTONS; i++) {
    held[i] = 0;
    settle[i] = 0;
  }
  settings_load();
  leds_draw();
  HID_init();
  knob = knob_state();

  while(1) {
    for(i = 0; i < BUTTONS; i++) {
      if(button_down(i) != held[i]) {
        if(++settle[i] >= DEBOUNCE_MS) {
          held[i] = !held[i];
          settle[i] = 0;
          set_code(button_code(i), held[i]);
          if(effect == EFFECT_PRESS) leds_dirty = 1;
        }
      } else {
        settle[i] = 0;
      }
    }

    now = knob_state();
    if(now != knob) {
      turn += KNOB_STEP[(knob << 2) | now];
      knob = now;
      if(now == 3) {                        // back at rest between clicks
        if(turn >= 2) tap(CODE_KNOB_RIGHT);
        else if(turn <= -2) tap(CODE_KNOB_LEFT);
        turn = 0;
      }
    }

    if(HID_cmdReady) handle_command();

    if(effect == EFFECT_RAINBOW && ++tick >= 30) {
      tick = 0;
      hue = hue >= 191 ? 0 : hue + 1;
      leds_dirty = 1;
    }
    if(leds_dirty) {
      leds_dirty = 0;
      leds_draw();
    }

    DLY_ms(1);
  }
}

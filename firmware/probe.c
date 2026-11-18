// ===================================================================================
// MiniMacPad pin probe for the CH552G six-key macro pad
// ===================================================================================
//
// A one-off test firmware that finds which chip pin each control is wired to. Every
// usable GPIO is an input with a pull-up; whenever one is pulled low (a key or the
// knob switch pressed, or a knob contact closing while it turns) the pad types an
// otherwise unassigned key code, 0xA5 plus the pin's index below, which nothing on
// the Mac reacts to and `minipad monitor --raw` shows. The one pin that never
// reports is the LED data line.
//
// Index:  0     1     2     3     4     5     6     7     8     9
// Pin:    P1.1  P1.4  P1.5  P1.6  P1.7  P3.0  P3.1  P3.2  P3.3  P3.4
//
// Pins already low at start-up are reported once, about a second after it.
// To flash again, short the SW2 pads while plugging in.

#include "src/config.h"
#include "src/system.h"
#include "src/gpio.h"
#include "src/delay.h"
#include "src/usb_hid.h"

// USB interrupt, handled by the library.
void USB_interrupt(void);
void USB_ISR(void) __interrupt(INT_NO_USB) {
  USB_interrupt();
}

#define PROBE_BASE 0xA5                     // first of ten unassigned usage codes

__xdata uint8_t report[8] = {1, 0, 0, 0, 0, 0, 0, 0};

// Which probed pins are low right now, one bit per index.
static uint16_t low_pins(void) {
  uint16_t v = 0;
  if(!PIN_read(P11)) v |= 1 << 0;
  if(!PIN_read(P14)) v |= 1 << 1;
  if(!PIN_read(P15)) v |= 1 << 2;
  if(!PIN_read(P16)) v |= 1 << 3;
  if(!PIN_read(P17)) v |= 1 << 4;
  if(!PIN_read(P30)) v |= 1 << 5;
  if(!PIN_read(P31)) v |= 1 << 6;
  if(!PIN_read(P32)) v |= 1 << 7;
  if(!PIN_read(P33)) v |= 1 << 8;
  if(!PIN_read(P34)) v |= 1 << 9;
  return v;
}

// Press and release one key code.
static void tap(uint8_t code) {
  report[3] = code;
  HID_sendReport(report, sizeof(report));
  DLY_ms(15);
  report[3] = 0;
  HID_sendReport(report, sizeof(report));
  DLY_ms(15);
}

// Report every pin set in `pins`.
static void report_pins(uint16_t pins) {
  uint8_t i;
  for(i = 0; i < 10; i++) {
    if(pins & (1 << i)) tap(PROBE_BASE + i);
  }
}

void main(void) {
  uint16_t last, now;

  CLK_config();
  DLY_ms(10);

  PIN_input_PU(P11);
  PIN_input_PU(P14);
  PIN_input_PU(P15);
  PIN_input_PU(P16);
  PIN_input_PU(P17);
  PIN_input_PU(P30);
  PIN_input_PU(P31);
  PIN_input_PU(P32);
  PIN_input_PU(P33);
  PIN_input_PU(P34);

  HID_init();
  DLY_ms(1000);                             // let the Mac finish setting the pad up

  last = low_pins();
  report_pins(last);

  while(1) {
    now = low_pins();
    report_pins(now & ~last);               // pins that just went low
    last = now;
    DLY_ms(2);
  }
}

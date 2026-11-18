// ===================================================================================
// MiniMacPad firmware configuration
// ===================================================================================

#pragma once

// Pin definitions, found with probe.c on the six-key 1189:8890 pad (keys numbered left
// to right, top to bottom; the knob sits above them). Every key closes to ground.
#define PIN_KEY1            P16
#define PIN_KEY2            P32
#define PIN_KEY3            P17
#define PIN_KEY4            P14
#define PIN_KEY5            P11
#define PIN_KEY6            P15
#define PIN_ENC_A           P31         // knob contact that closes first turning right
#define PIN_ENC_B           P30
#define PIN_ENC_SW          P33         // knob press
#define PIN_NEO             P34         // data input of the key LEDs

// LED configuration
#define NEO_COUNT           6           // number of LEDs in the chain
#define NEO_GRB                         // colour order: NEO_GRB or NEO_RGB

// USB device descriptor: the pad's original id, so MiniMacPad keeps finding it.
#define USB_VENDOR_ID       0x1189      // VID
#define USB_PRODUCT_ID      0x8890      // PID
#define USB_DEVICE_VERSION  0x0203      // MiniMacPad firmware 2.3 (BCD); the app reads it

// USB configuration descriptor
#define USB_MAX_POWER_mA    150         // max power in mA

// USB descriptor strings
#define MANUFACTURER_STR    'M','i','n','i','M','a','c','P','a','d'
#define PRODUCT_STR         'M','i','n','i','M','a','c','P','a','d'
#define SERIAL_STR          'C','H','5','5','2'
#define INTERFACE_STR       'H','I','D','-','K','e','y','b','o','a','r','d'

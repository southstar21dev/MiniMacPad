// ===================================================================================
// USB HID Functions for CH551, CH552 and CH554
// ===================================================================================

#pragma once
#include <stdint.h>
#include "usb_handler.h"

// MiniMacPad vendor commands from the host: report id 5, then 7 bytes.
#define HID_CMD_REPORT_ID 5
#define HID_CMD_LEN       8
extern volatile __bit HID_cmdReady;           // a command is waiting in HID_cmd
extern __xdata uint8_t HID_cmd[HID_CMD_LEN];

void HID_init(void);                                      // setup USB-HID
void HID_sendReport(__xdata uint8_t* buf, uint8_t len);   // send HID report

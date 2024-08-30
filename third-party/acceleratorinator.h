#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define CRING_ACC_VID 49374

#define CRING_ACC_PID 51966

#define CRING_ACC_BOUT_EP 1

#define CRING_ACC_BIN_EP 129

/**
 * Operation went ok
 */
#define CRING_EOK 0

/**
 * Operation has already happened so this call in invalid
 */
#define CRING_EALREADY -1

/**
 * Some parameter is invalid
 */
#define CRING_EINVAL -2

/**
 * The search yielded no valid result
 */
#define CRING_ENOTPRESENT -3

/**
 * There was an error interacting with the USB
 */
#define CRING_EUSB -4

/**
 * Unknown acceleratorinator error
 */
#define CRING_EACC_UNKNOWN -100

/**
 * Unsupported compression
 */
#define CRING_EACC_UNSUP_COMP -101

/**
 * Parse failure
 */
#define CRING_EACC_PARSE -102

typedef struct CringUsbConnection CringUsbConnection;

/**
 * Create the USB structure
 */
int cring_usb_create(struct CringUsbConnection **usb);

/**
 * Free the USB structure
 */
int cring_usb_free(struct CringUsbConnection **usb);

/**
 * Connect the USB to the first interface
 */
int cring_usb_connect(struct CringUsbConnection *usb, uint16_t vendor_id, uint16_t product_id);

/**
 * Send a bulk out message. The endpoint must *not* have its top-bit (`0x80`) set
 */
int cring_usb_bulk_out(struct CringUsbConnection *usb,
                       uint8_t ep,
                       const uint8_t *data,
                       uintptr_t len);

/**
 * Send a bulk in message. The endpoint must have its top-bit (`0x80`) set
 */
int cring_usb_bulk_in(struct CringUsbConnection *usb, uint8_t ep, uint8_t *data, uintptr_t len);

int cring_acc_send_bmp(struct CringUsbConnection *usb, uint8_t *bmp_data, uintptr_t bmp_len);

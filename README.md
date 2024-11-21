# Acceleratorinator

This project is a USB video accelerator project for the nRF52840.

Inverting the colors of a BMP image is a lot of work, best done by an external device.

Note: Dynamic linking is used! So when executing the CLI binary,
make sure the `libacceleratorinator.so` file is in your working directory.
`cargo run` is set up to also 'just' work.

Only a linux .so file is provided, so make sure to use Linux.

## USB setup

### Linux

1. Copy the file `99-accelatorinator.rules` to `/etc/udev/rules.d/`.
2. Run `udevadm control --reload` to ensure the new rules are used.
3. Run `udevadm trigger` to ensure the new rules are applied to already added devices.

### Windows

Care has been taken so the USB device shows up as a WinUSB device.
No manual actions required.

### MacOS

No manual actions required.

## Suggested tools to use

- `nm`
- `objdump -C -D <PATH>`
- `readelf --debug-dump <PATH>`
- Debugger
- Ghidra

package app.tauri.serialplugin.models

import com.hoho.android.usbserial.driver.UsbSerialPort

enum class DataBits(val value: Int) {
    FIVE(5),
    SIX(6),
    SEVEN(7),
    EIGHT(8)
}

enum class FlowControl {
    NONE,
    SOFTWARE,
    HARDWARE
}

enum class Parity(val value: Int) {
    NONE(UsbSerialPort.PARITY_NONE),
    ODD(UsbSerialPort.PARITY_ODD),
    EVEN(UsbSerialPort.PARITY_EVEN)
}

enum class StopBits(val value: Int) {
    ONE(UsbSerialPort.STOPBITS_1),
    TWO(UsbSerialPort.STOPBITS_2)
}

enum class ClearBuffer {
    INPUT,
    OUTPUT,
    ALL
}

data class SerialPortConfig(
    val path: String,
    val baudRate: Int = 9600,
    val dataBits: DataBits = DataBits.EIGHT,
    val flowControl: FlowControl = FlowControl.NONE,
    val parity: Parity = Parity.NONE,
    val stopBits: StopBits = StopBits.ONE,
    val timeout: Int = 1000
)
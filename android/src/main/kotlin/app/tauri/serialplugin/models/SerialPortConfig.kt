package app.tauri.serialplugin.models

import com.hoho.android.usbserial.driver.UsbSerialPort

enum class DataBits(val value: Int) {
    FIVE(5),
    SIX(6),
    SEVEN(7),
    EIGHT(8);

    companion object {
        fun fromValue(value: Int): DataBits {
            return DataBits.entries.find { it.value == value } ?: EIGHT
        }
    }
}

enum class FlowControl {
    NONE,
    SOFTWARE,
    HARDWARE;

    companion object {
        fun fromValue(value: Int): FlowControl {
            return when (value) {
                1 -> SOFTWARE
                2 -> HARDWARE
                else -> NONE
            }
        }
    }
}

enum class Parity(val value: Int) {
    NONE(UsbSerialPort.PARITY_NONE),
    ODD(UsbSerialPort.PARITY_ODD),
    EVEN(UsbSerialPort.PARITY_EVEN);

    companion object {
        fun fromValue(value: Int): Parity {
            return Parity.entries.find { it.value == value } ?: NONE
        }
    }
}

enum class StopBits(val value: Int) {
    ONE(UsbSerialPort.STOPBITS_1),
    TWO(UsbSerialPort.STOPBITS_2);

    companion object {
        fun fromValue(value: Int): StopBits {
            return StopBits.entries.find { it.value == value } ?: ONE
        }
    }
}

enum class ClearBuffer {
    INPUT,
    OUTPUT,
    ALL;

    companion object {
        fun fromValue(value: String): ClearBuffer {
            return when (value.lowercase()) {
                "input" -> INPUT
                "output" -> OUTPUT
                "all" -> ALL
                else -> INPUT
            }
        }
    }
}

data class SerialPortConfig(
    var path: String,
    var baudRate: Int = 9600,
    var dataBits: DataBits = DataBits.EIGHT,
    var flowControl: FlowControl = FlowControl.NONE,
    var parity: Parity = Parity.NONE,
    var stopBits: StopBits = StopBits.ONE,
    var timeout: Int = 1000
)

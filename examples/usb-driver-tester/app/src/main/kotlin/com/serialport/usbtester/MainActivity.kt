package com.serialport.usbtester

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.ListView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {

    private lateinit var logView: TextView
    private lateinit var deviceList: ListView
    private val devices = mutableListOf<UsbDevice>()
    private var selectedIndex = -1
    private var pendingDevice: UsbDevice? = null

    private val permissionReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != ACTION_USB_PERMISSION) return
            val dev = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
            if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false) && dev != null) {
                runDeviceTest(dev)
            } else {
                appendLog("USB permission denied for ${dev?.deviceName}")
            }
            pendingDevice = null
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        logView = findViewById(R.id.logView)
        deviceList = findViewById(R.id.deviceList)
        findViewById<Button>(R.id.refreshBtn).setOnClickListener { refreshDevices() }
        findViewById<Button>(R.id.testBtn).setOnClickListener { runSelfTest() }
        findViewById<Button>(R.id.shareBtn).setOnClickListener { shareLog() }

        deviceList.setOnItemClickListener { _, _, position, _ ->
            selectedIndex = position
            val dev = devices[position]
            appendLog("Selected ${dev.deviceName} vid=${dev.vendorId} pid=${dev.productId}")
            runDeviceTestWithPermission(dev)
        }

        val filter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(permissionReceiver, filter, RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(permissionReceiver, filter)
        }

        refreshDevices()
        handleUsbIntent(intent)
    }

    override fun onDestroy() {
        unregisterReceiver(permissionReceiver)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleUsbIntent(intent)
    }

    private fun handleUsbIntent(intent: Intent?) {
        val dev = intent?.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
        if (dev != null) {
            appendLog("USB attached: ${dev.deviceName}")
            refreshDevices()
        }
    }

    private fun refreshDevices() {
        val mgr = getSystemService(USB_SERVICE) as UsbManager
        devices.clear()
        devices.addAll(mgr.deviceList.values)
        val labels = devices.map { d ->
            val driver = nativeProbeDriver(d.vendorId, d.productId)
            "${d.deviceName} ${hex(d.vendorId)}:${hex(d.productId)} → $driver"
        }
        deviceList.adapter = ArrayAdapter(this, android.R.layout.simple_list_item_1, labels)
        appendLog("Found ${devices.size} device(s)")
    }

    private fun runSelfTest() {
        appendLog("--- fake matrix start ---")
        appendLog(nativeRunSelfTest())
        appendLog("--- fake matrix end ---")
    }

    private fun runDeviceTestWithPermission(dev: UsbDevice) {
        val mgr = getSystemService(USB_SERVICE) as UsbManager
        if (!mgr.hasPermission(dev)) {
            pendingDevice = dev
            val pi = PendingIntent.getBroadcast(
                this,
                0,
                Intent(ACTION_USB_PERMISSION),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
            )
            mgr.requestPermission(dev, pi)
            appendLog("Requesting USB permission for ${dev.deviceName}")
            return
        }
        runDeviceTest(dev)
    }

    private fun runDeviceTest(dev: UsbDevice) {
        val mgr = getSystemService(USB_SERVICE) as UsbManager
        val conn = mgr.openDevice(dev) ?: run {
            appendLog("FAIL openDevice ${dev.deviceName}")
            return
        }
        appendLog("--- real fd test ${dev.deviceName} ---")
        try {
            appendLog(nativeOpenAndTest(conn.fileDescriptor, dev.vendorId, dev.productId))
        } finally {
            conn.close()
        }
        appendLog("--- real fd test end ---")
    }

    private fun shareLog() {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, logView.text.toString())
        }
        startActivity(Intent.createChooser(send, "Share log"))
    }

    private fun appendLog(line: String) {
        logView.append(line)
        logView.append("\n")
    }

    private fun hex(v: Int) = String.format("%04X", v)

    private external fun nativeProbeDriver(vendorId: Int, productId: Int): String
    private external fun nativeRunSelfTest(): String
    private external fun nativeOpenAndTest(fd: Int, vendorId: Int, productId: Int): String

    companion object {
        private const val ACTION_USB_PERMISSION = "com.serialport.usbtester.USB_PERMISSION"

        init {
            System.loadLibrary("usb_driver_tester")
        }
    }
}

@file:Suppress("unused")

package app.tauri.serialplugin

import android.app.Activity
import android.app.Application
import android.os.Bundle
import android.webkit.WebView
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin
import app.tauri.serialplugin.manager.UsbBridge

@TauriPlugin
class SerialPlugin(private val activity: Activity) : Plugin(activity) {
    private lateinit var usb: UsbBridge
    private var destroyCb: Application.ActivityLifecycleCallbacks? = null

    override fun load(webView: WebView) {
        super.load(webView)
        usb = UsbBridge(activity.applicationContext)
        UsbNative.bind(usb)
        val app = activity.application
        destroyCb = object : Application.ActivityLifecycleCallbacks {
            override fun onActivityDestroyed(a: Activity) {
                if (a !== activity) return
                usb.shutdown()
                MobileBridge.onAppDestroy()
                app.unregisterActivityLifecycleCallbacks(this)
                destroyCb = null
            }
            override fun onActivityCreated(a: Activity, s: Bundle?) {}
            override fun onActivityStarted(a: Activity) {}
            override fun onActivityResumed(a: Activity) {}
            override fun onActivityPaused(a: Activity) {}
            override fun onActivityStopped(a: Activity) {}
            override fun onActivitySaveInstanceState(a: Activity, s: Bundle) {}
        }
        app.registerActivityLifecycleCallbacks(destroyCb!!)
    }
}

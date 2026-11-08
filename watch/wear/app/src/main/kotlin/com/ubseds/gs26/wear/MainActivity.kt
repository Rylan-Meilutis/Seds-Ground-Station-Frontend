package com.ubseds.gs26.wear

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.ubseds.gs26.wear.ui.WatchApp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val link by lazy { PhoneLink.get(this) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { WatchApp(link) }
        // While foreground, tell the phone every 15 s so it keeps the fast rate.
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.RESUMED) {
                while (true) {
                    link.sendActivity(foreground = true)
                    delay(ACTIVITY_PERIOD_MS)
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        link.start()
    }

    override fun onPause() {
        link.sendActivity(foreground = false)
        super.onPause()
    }

    override fun onStop() {
        link.stop()
        super.onStop()
    }

    private companion object {
        const val ACTIVITY_PERIOD_MS = 15_000L
    }
}

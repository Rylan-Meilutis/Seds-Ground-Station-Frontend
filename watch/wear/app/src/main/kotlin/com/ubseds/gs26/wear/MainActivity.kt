package com.ubseds.gs26.wear

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.ubseds.gs26.wear.ui.WatchApp
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val link by lazy { PhoneLink.get(this) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        takeOpenPage(intent)
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
        // While foreground and the phone's data is not fresh, read the
        // station directly (plain HTTP polls, nothing in the background).
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.RESUMED) {
                try {
                    while (true) delay(link.pollDirectIfNeeded())
                } finally {
                    link.direct.setPolling(false)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        takeOpenPage(intent)
    }

    /** The push-to-talk tile and the voice notification open a page (`voice`: and join). */
    private fun takeOpenPage(intent: Intent?) {
        val page = intent?.getStringExtra(EXTRA_OPEN_PAGE) ?: return
        intent.removeExtra(EXTRA_OPEN_PAGE)
        openPage.value = page
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

    companion object {
        private const val ACTIVITY_PERIOD_MS = 15_000L
        const val EXTRA_OPEN_PAGE = "open_page"

        /** A page to select once (set by the tile / notification, consumed by the pager). */
        val openPage = MutableStateFlow<String?>(null)
    }
}

package com.breezone.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import com.breezone.app.data.ConnectLinks
import androidx.lifecycle.lifecycleScope
import com.breezone.app.ui.App
import com.breezone.app.widgets.refreshWidgets
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val askLocalNetwork = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        requestLocalNetworkAccess()
        if (savedInstanceState == null) openLink(intent)
        setContent { App() }
    }

    // Back on the home screen: the widgets show what was just changed in the app.
    override fun onStop() {
        super.onStop()
        lifecycleScope.launch { refreshWidgets(applicationContext) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openLink(intent)
    }

    private fun openLink(intent: Intent?) {
        intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString?.let { ConnectLinks.open(it) }
    }

    /**
     * Android 17 blocks connections to the local network unless the user allows it. Breezone
     * servers are usually reached at home on a local address, so ask once at startup (the system
     * does not ask again once the user has answered).
     */
    private fun requestLocalNetworkAccess() {
        if (Build.VERSION.SDK_INT < 37) return
        val permission = Manifest.permission.ACCESS_LOCAL_NETWORK
        if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
            askLocalNetwork.launch(permission)
        }
    }
}

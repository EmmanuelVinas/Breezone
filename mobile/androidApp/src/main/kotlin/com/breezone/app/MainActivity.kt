package com.breezone.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import com.breezone.app.ui.App

class MainActivity : ComponentActivity() {
    private val askLocalNetwork = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        requestLocalNetworkAccess()
        setContent { App() }
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

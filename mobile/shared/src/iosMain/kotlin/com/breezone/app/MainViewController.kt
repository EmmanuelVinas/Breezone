package com.breezone.app

import androidx.compose.ui.window.ComposeUIViewController
import com.breezone.app.data.ConnectLinks
import com.breezone.app.ui.App

@Suppress("FunctionName", "unused") // Called from Swift.
fun MainViewController() = ComposeUIViewController { App() }

/** A link the system opened the app with (the QR code of the web UI). */
@Suppress("unused") // Called from Swift.
fun openLink(url: String): Boolean = ConnectLinks.open(url)

package com.breezone.app

import androidx.compose.ui.window.ComposeUIViewController
import com.breezone.app.ui.App

@Suppress("FunctionName", "unused") // Called from Swift.
fun MainViewController() = ComposeUIViewController { App() }

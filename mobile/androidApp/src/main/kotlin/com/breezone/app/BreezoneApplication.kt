package com.breezone.app

import android.app.Application
import com.breezone.app.di.initKoin
import org.koin.android.ext.koin.androidContext

class BreezoneApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        initKoin { androidContext(this@BreezoneApplication) }
    }
}

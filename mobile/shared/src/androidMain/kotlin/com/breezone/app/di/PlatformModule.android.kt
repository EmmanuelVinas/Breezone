package com.breezone.app.di

import com.breezone.app.data.SecureStore
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.engine.okhttp.OkHttp
import org.koin.android.ext.koin.androidContext
import org.koin.dsl.module
import java.util.concurrent.TimeUnit

actual val platformModule = module {
    single { SecureStore(androidContext()) }
    single<HttpClientEngine> {
        OkHttp.create {
            config {
                connectTimeout(10, TimeUnit.SECONDS)
                // The server pings the WebSocket every 25 s.
                readTimeout(70, TimeUnit.SECONDS)
            }
        }
    }
}

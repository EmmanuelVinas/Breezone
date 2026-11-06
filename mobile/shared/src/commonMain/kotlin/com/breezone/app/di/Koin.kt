package com.breezone.app.di

import com.breezone.app.data.FzJson
import com.breezone.app.data.Hub
import io.ktor.client.HttpClient
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.koin.core.context.startKoin
import org.koin.core.module.Module
import org.koin.dsl.KoinAppDeclaration
import org.koin.dsl.module

/** Provides `SecureStore` and the native `HttpClientEngine`. */
expect val platformModule: Module

val appModule = module {
    single {
        HttpClient(get<HttpClientEngine>()) {
            install(ContentNegotiation) { json(FzJson) }
            install(WebSockets)
        }
    }
    single { Hub(get(), get(), CoroutineScope(SupervisorJob() + Dispatchers.Default)) }
}

fun initKoin(config: KoinAppDeclaration? = null) {
    startKoin {
        config?.invoke(this)
        modules(platformModule, appModule)
    }
}

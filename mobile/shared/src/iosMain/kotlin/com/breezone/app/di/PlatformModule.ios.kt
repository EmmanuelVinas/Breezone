package com.breezone.app.di

import com.breezone.app.data.SecureStore
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.engine.darwin.Darwin
import org.koin.dsl.module

actual val platformModule = module {
    single { SecureStore() }
    single<HttpClientEngine> { Darwin.create() }
}

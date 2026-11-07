@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package com.breezone.app.di

import com.breezone.app.data.SecureStore
import com.breezone.app.data.TlsTrust
import com.breezone.app.data.fingerprint
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.engine.darwin.Darwin
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.usePinned
import org.koin.dsl.module
import platform.Foundation.CFBridgingRelease
import platform.Foundation.NSData
import platform.Foundation.NSURLAuthenticationMethodServerTrust
import platform.Foundation.NSURLCredential
import platform.Foundation.NSURLSessionAuthChallengePerformDefaultHandling
import platform.Foundation.NSURLSessionAuthChallengeUseCredential
import platform.Foundation.credentialForTrust
import platform.Foundation.serverTrust
import platform.Security.SecCertificateCopyData
import platform.Security.SecTrustEvaluateWithError
import platform.Security.SecTrustGetCertificateAtIndex
import platform.posix.memcpy

actual val platformModule = module {
    single { SecureStore() }
    single<HttpClientEngine> {
        Darwin.create {
            // Usual checks, except for a certificate the user trusted for that server
            // (see `TlsTrust`); the certificate presented is recorded to show it to the user.
            handleChallenge { _, _, challenge, completionHandler ->
                val space = challenge.protectionSpace
                val trust = space.serverTrust
                if (space.authenticationMethod != NSURLAuthenticationMethodServerTrust || trust == null) {
                    completionHandler(NSURLSessionAuthChallengePerformDefaultHandling, null)
                    return@handleChallenge
                }
                val host = space.host
                val port = space.port.toInt()
                val der = leafCertificate(trust)
                if (der != null) TlsTrust.record(host, port, der)
                val trusted = der != null && !SecTrustEvaluateWithError(trust, null) && TlsTrust.pinned(host, port) == fingerprint(der)
                if (trusted) {
                    completionHandler(NSURLSessionAuthChallengeUseCredential, NSURLCredential.credentialForTrust(trust))
                } else {
                    completionHandler(NSURLSessionAuthChallengePerformDefaultHandling, null)
                }
            }
        }
    }
}

@OptIn(ExperimentalForeignApi::class)
private fun leafCertificate(trust: platform.Security.SecTrustRef): ByteArray? {
    @Suppress("DEPRECATION")
    val cert = SecTrustGetCertificateAtIndex(trust, 0) ?: return null
    val data = CFBridgingRelease(SecCertificateCopyData(cert)) as? NSData ?: return null
    val size = data.length.toInt()
    if (size == 0) return null
    return ByteArray(size).also { bytes -> bytes.usePinned { memcpy(it.addressOf(0), data.bytes, data.length) } }
}

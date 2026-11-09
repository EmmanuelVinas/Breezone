package com.breezone.app.di

import com.breezone.app.data.SecureStore
import com.breezone.app.data.TlsTrust
import com.breezone.app.data.fingerprint
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.engine.okhttp.OkHttp
import okhttp3.internal.tls.OkHostnameVerifier
import org.koin.android.ext.koin.androidContext
import org.koin.dsl.module
import java.net.Socket
import java.security.KeyStore
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509ExtendedTrustManager

actual val platformModule = module {
    single { SecureStore(androidContext()) }
    single<HttpClientEngine> {
        val trust = PinningTrustManager()
        val tls = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trust), null) }
        OkHttp.create {
            config {
                connectTimeout(10, TimeUnit.SECONDS)
                readTimeout(70, TimeUnit.SECONDS)
                sslSocketFactory(tls.socketFactory, trust)
                hostnameVerifier(PinningHostnameVerifier)
            }
        }
    }
}

/**
 * Usual certificate checks, except for a certificate the user trusted for that server
 * (see `TlsTrust`). Records the certificate each server presents, to show it to the user.
 */
private class PinningTrustManager : X509ExtendedTrustManager() {
    private val system: X509ExtendedTrustManager = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
        .apply { init(null as KeyStore?) }
        .trustManagers.filterIsInstance<X509ExtendedTrustManager>().first()

    override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String, socket: Socket) {
        val host = (socket as? SSLSocket)?.handshakeSession?.peerHost ?: socket.inetAddress?.hostName
        if (host != null && chain.isNotEmpty()) {
            TlsTrust.record(host, socket.port, chain[0].encoded)
            if (TlsTrust.pinned(host, socket.port) == fingerprint(chain[0].encoded)) return
        }
        system.checkServerTrusted(chain, authType, socket)
    }

    override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String, engine: SSLEngine) {
        val host = engine.peerHost
        if (host != null && chain.isNotEmpty()) {
            TlsTrust.record(host, engine.peerPort, chain[0].encoded)
            if (TlsTrust.pinned(host, engine.peerPort) == fingerprint(chain[0].encoded)) return
        }
        system.checkServerTrusted(chain, authType, engine)
    }

    override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) = system.checkServerTrusted(chain, authType)
    override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) = system.checkClientTrusted(chain, authType)
    override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String, socket: Socket) = system.checkClientTrusted(chain, authType, socket)
    override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String, engine: SSLEngine) = system.checkClientTrusted(chain, authType, engine)
    override fun getAcceptedIssuers(): Array<X509Certificate> = system.acceptedIssuers
}

/** Usual name check, except for a certificate the user trusted for that server. */
private object PinningHostnameVerifier : HostnameVerifier {
    override fun verify(hostname: String, session: javax.net.ssl.SSLSession): Boolean {
        if (OkHostnameVerifier.verify(hostname, session)) return true
        val leaf = runCatching { session.peerCertificates.firstOrNull()?.encoded }.getOrNull() ?: return false
        TlsTrust.record(hostname, session.peerPort, leaf)
        return TlsTrust.pinned(hostname, session.peerPort) == fingerprint(leaf)
    }
}

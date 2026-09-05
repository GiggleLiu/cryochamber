package com.cryochamber.console

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class CredentialValue { lateinit var value: String }

/** One fixed encrypted record. The encryption key never leaves Android Keystore. */
@TauriPlugin
class CredentialsPlugin(private val activity: Activity) : Plugin(activity) {
    private val alias = "com.cryochamber.console.hub-tokens"
    private fun preferences() = activity.getSharedPreferences("hub-credentials", Context.MODE_PRIVATE)
    private fun key(create: Boolean): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        check(create) { "Hub credential key is unavailable; add the hub again on this device" }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build())
            generateKey()
        }
    }

    @Command
    fun load(invoke: Invoke) {
        try {
            val encoded = preferences().getString("record", null)
            val result = JSObject()
            if (encoded != null) {
                val bytes = Base64.decode(encoded, Base64.NO_WRAP)
                check(bytes.size >= 28) { "Invalid encrypted hub credentials" }
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
                result.put("value", String(cipher.doFinal(bytes.copyOfRange(12, bytes.size)), Charsets.UTF_8))
            }
            invoke.resolve(result)
        } catch (_: Exception) {
            invoke.reject("Cannot unlock hub credentials on this device. Restore access to the device key or re-add the hub.")
        }
    }

    @Command
    fun save(invoke: Invoke) {
        try {
            val value = invoke.parseArgs(CredentialValue::class.java).value
            check(value.toByteArray(Charsets.UTF_8).size <= 1_048_576) { "Credentials exceed 1 MiB" }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key(true))
            val encrypted = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            check(preferences().edit().putString("record", Base64.encodeToString(encrypted, Base64.NO_WRAP)).commit()) {
                "Cannot persist hub credentials"
            }
            invoke.resolve()
        } catch (_: Exception) {
            invoke.reject("Cannot save hub credentials in device storage")
        }
    }
}

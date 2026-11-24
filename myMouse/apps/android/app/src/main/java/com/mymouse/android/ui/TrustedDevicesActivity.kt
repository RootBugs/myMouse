package com.mymouse.android.ui

import android.os.Bundle
import android.view.ViewGroup
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.mymouse.android.MyMouseApp
import com.mymouse.android.databinding.ActivityTrustedDevicesBinding

class TrustedDevicesActivity : AppCompatActivity() {
    private lateinit var binding: ActivityTrustedDevicesBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityTrustedDevicesBinding.inflate(layoutInflater)
        setContentView(binding.root)
        binding.trustedList.layoutManager = LinearLayoutManager(this)
        refresh()
    }

    private fun refresh() {
        val devices = (application as MyMouseApp).trustStore.list()
        binding.trustedEmpty.visibility = if (devices.isEmpty()) android.view.View.VISIBLE else android.view.View.GONE
        binding.trustedList.adapter = object : RecyclerView.Adapter<VH>() {
            override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): VH {
                val tv = TextView(parent.context).apply {
                    setPadding(16, 24, 16, 24)
                    textSize = 14f
                }
                return VH(tv)
            }
            override fun getItemCount(): Int = devices.size
            override fun onBindViewHolder(holder: VH, position: Int) {
                val d = devices[position]
                holder.textView.text =
                    "${d.name}\nID: ${d.deviceId}\nCaps: input=${d.capabilities.input}, clipboard=${d.capabilities.clipboard}, files=${d.capabilities.files}, screen=${d.capabilities.screen}\nFingerprint: ${d.fingerprint ?: "(not pinned in scaffold)"}\nTap to revoke"
                holder.textView.setOnClickListener {
                    (application as MyMouseApp).trustStore.revoke(d.deviceId)
                    refresh()
                }
            }
        }
    }

    class VH(val textView: TextView) : RecyclerView.ViewHolder(textView)
}

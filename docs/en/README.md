**English** | [简体中文](https://github.com/Seyud/FreePPS/blob/main/docs/README.md)

# FreePPS 🔋⚡

<img src="../logo.png" style="width: 96px;" alt="logo">

**Enable public PPS fast charging support for Xiaomi/Mi devices!**

[![Version](https://img.shields.io/github/v/release/Seyud/FreePPS?logo=github)](https://github.com/Seyud/FreePPS/releases/latest)
[![GitHub Downloads](https://img.shields.io/github/downloads/Seyud/FreePPS/total?logo=github&logoColor=green)](https://github.com/Seyud/FreePPS/releases)
[![Language](https://img.shields.io/badge/language-Rust-orange?logo=rust&logoColor=orange)](https://www.rust-lang.org/)
[![Telegram](https://img.shields.io/badge/Telegram-Group-2AABEE?logo=telegram&logoColor=white)](https://t.me/HyperChargePPS)

## ✨ Module Overview

FreePPS is a module specifically designed for Xiaomi/Mi devices that can **unlock and enable public PPS (Programmable Power Supply) fast charging protocol support**, giving your device better compatibility!

> ⚠️ **Important Note**: Automatic detection is currently verified only on Qualcomm devices. On MediaTek, Automatic mode continues to behave like Force Public PPS.

> 💡 **Special Thanks**: Node provided by Coolapk @低线阻狂魔 and Coolapk @花橋桥

## 🚀 Key Features

- ✅ **PPS Protocol Unlock** - Enable public PPS fast charging support
- 🔄 **Automatic protocol detection (Qualcomm)** - Prefer Xiaomi charging on attachment, then perform one software reconnect for public-PPS-only chargers
- 🎛️ **Three modes** - The module action button cycles through Xiaomi Priority, Force Public PPS, and Automatic Detection
- 🔛 **Temporary Control** - Use module switches for compatibility switching of PPS support status

Automatic mode uses deadlines only during the brief negotiation after attachment. While idle, it sleeps until a kernel event instead of polling. Leave the module in Automatic mode and disconnect normally before changing chargers so the next attachment starts with Xiaomi authentication enabled.

## 🙏 Acknowledgments

- **Coolapk @低线阻狂魔**, **Coolapk @花橋桥** - Node
- **All test users** - Effective feedback and suggestions


---

**⚡ Let every Xiaomi device enjoy free charging experience!** 🔋

> 💝 If this module has been helpful to you, please give it a Star to show your support!

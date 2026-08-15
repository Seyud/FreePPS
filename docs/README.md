**简体中文** | [English](https://github.com/Seyud/FreePPS/blob/main/docs/en/README.md)

# FreePPS 🔋⚡

<img src="logo.png" style="width: 96px;" alt="logo">

**让米系设备享受公版PPS快充支持！**

[![Version](https://img.shields.io/github/v/release/Seyud/FreePPS?logo=github)](https://github.com/Seyud/FreePPS/releases/latest)
[![GitHub Downloads](https://img.shields.io/github/downloads/Seyud/FreePPS/total?logo=github&logoColor=green)](https://github.com/Seyud/FreePPS/releases)
[![Language](https://img.shields.io/badge/language-Rust-orange?logo=rust&logoColor=orange)](https://www.rust-lang.org/)
[![QQ群](https://img.shields.io/badge/QQ群-1040448481-12B7F5?logo=qq&logoColor=white)](https://qun.qq.com/universal-share/share?ac=1&authKey=yGST4VNx53EILHT9hGSsCp31dMjknUn2Sk7YQpor07eDDzT24a1DtLnOmLqqlwgU&busi_data=eyJncm91cENvZGUiOiIxMDQwNDQ4NDgxIiwidG9rZW4iOiJHWHkyQ2E2TVMyZWU0WnNRU01UN2c2YjhuS0Vhb3Q3VERFZ0VTekxFVlorR3c4eVBYM2hDTXNuTjBRSm83T0FlIiwidWluIjoiMTEwNTc4MzAzMyJ9&data=qlkmodlW6ZB2dDVXRmbCMk_0FAgwp5Dw-UBtThSD5KUx4erJDe0cNBnEJnYqaFnGRLQ_asSN9AQOuG5Z6TxX0A&svctype=4&tempid=h5_group_info)
[![Telegram群组](https://img.shields.io/badge/Telegram-Group-2AABEE?logo=telegram&logoColor=white)](https://t.me/HyperChargePPS)

## ✨ 模块简介

FreePPS 是一个专为米系设备设计的模块，能够**解锁并启用公版 PPS（Programmable Power Supply）快充协议支持**，让你的设备享受更好的兼容性！

> ⚠️ **重要注意事项**：自动识别目前仅在高通平台验证。联发科平台选择自动模式时仍按“锁定公版 PPS”处理。

> 💡 **特别感谢**：酷安@低线阻狂魔、酷安@花橋桥 提供的节点

## 🚀 主要功能

- ✅ **PPS协议解锁** - 启用公版PPS快充支持
- 🔄 **自动协议识别（高通）** - 插入时优先尝试小米协议；若只检测到公版 PD PPS，则自动执行一次软件重连并启用公版 PPS
- 🎛️ **三种模式** - 模块操作按钮依次切换“小米协议优先 → 锁定公版 PPS → 自动识别”
- 🔛 **临时控制** - 使用模块开关进行兼容性切换PPS支持状态

自动模式仅在每次插入后的短暂协商阶段使用定时器，空闲时由内核事件唤醒，不进行周期轮询。为保证下一次插入能优先完成小米认证，请保持自动模式并在切换充电器前正常拔出当前充电器。


## 🙏 致谢

- **酷安@低线阻狂魔**、**酷安@花橋桥** - 提供节点
- **所有测试用户** - 有效的反馈和建议


---

**⚡ 让每一台小米设备都享受自由充电体验！** 🔋

> 💝 如果这个模块对你有帮助，可以给个 Star 支持一下！

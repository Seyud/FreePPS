---
name: 问题反馈
about: 报告 FreePPS 模块的问题
title: ''
labels: ''
assignees: ''

---

### 机型：
> 请填写您的设备型号（如：Xiaomi 14 Ultra）

### 模块版本：
> 请填写您使用的 FreePPS 模块版本号

### 模块模式:
> 请选择或填写您使用的模块模式
> - [ ] 锁定PPS支持
> - [ ] 协议自动识别
> - [ ] PPS已暂停

### 充电头铭牌：
> 请填写您的充电器铭牌信息（如：MDY-16-EA / 120W）
> 或拍照充电头铭牌并在此处说明

### 日志：
> 请附上模块日志文件
> 日志路径：`/data/adb/modules/FreePPS/FreePPS.log`

### 问题描述：
> 请详细描述您遇到的问题，包括：
> - 问题表现
> - 触发条件
> - 预期行为
> - 实际行为

---

## 日志反馈方法

1. 在 `/data/adb/modules/FreePPS/` 目录中创建名为 `debug` 的空文件
2. 重启设备
3. 充一次电（让模块记录充电日志）
4. 获取日志文件：`/data/adb/modules/FreePPS/FreePPS.log`
5. 将日志文件附在issue中发送

> 提示：创建debug文件后，模块会记录更详细的调试信息。建议在完成日志收集后删除debug文件以避免持续记录日志。

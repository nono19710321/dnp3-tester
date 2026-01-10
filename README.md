# DNP3 Protocol Tester

<div align="center">

**IEEE 1815-2012 Compliant DNP3 Testing Tool**

![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Status](https://img.shields.io/badge/status-active-success.svg)

</div>

## 🚀 快速开始

### 运行应用
```bash
cargo run --release
```

浏览器将自动打开 `http://127.0.0.1:8080`

### 编译发布版本
```bash
cargo build --release
./target/release/dnp3_tester
```

### 生成 aarch64 静态（零依赖）可执行文件（GitHub Actions / Cross-build）

推荐在 CI 中使用 musl-cross 镜像交叉编译以生成 `aarch64-unknown-linux-musl` 静态二进制。仓库包含一个 workflow `.github/workflows/build-aarch64-musl.yml`，会在 push 或手动触发时构建并上传 artifact。

本地快速尝试（在 Linux 主机上）：

```bash
# 1) 安装目标（在本机安装 musl 工具链可能更复杂，推荐使用 CI 或 Docker）
rustup target add aarch64-unknown-linux-musl

# 2) 在支持 musl 的交叉环境中构建，例如使用 messense 的 musl-cross docker image:
docker run --rm -v "$PWD":/work -w /work messense/rust-musl-cross:aarch64-1.70.0 bash -lc "cargo build --target aarch64-unknown-linux-musl --release && cp target/aarch64-unknown-linux-musl/release/dnp3_tester ./dnp3_tester-aarch64-musl"

# 生成的文件: ./dnp3_tester-aarch64-musl
```

## ✨ 功能特性

### 💻 双模式支持
- **Outstation (模拟器)** - 模拟DNP3设备，响应Master轮询和控制命令
- **Master (调试器)** - 主站模式，发送读取和控制命令

### 📡 通信协议
- ✅ TCP Client
- ✅ TCP Server  
- ✅ UDP
- ✅ Serial (RS-232/485)
- ✅ TLS (安全连接)

### 🎛️ 数据点类型
- **Binary Input** (BI) - 开关量输入
- **Binary Output** (BO) - 开关量输出  
- **Analog Input** (AI) - 模拟量输入
- **Analog Output** (AO) - 模拟量输出
- **Counter** - 计数器

### 🕹️ 控制操作
- **Direct Operate (DBO)** - 直接操作
- **Select Before Operate (SBO)** - 先选择后操作
- 支持二进制控制 (ON/OFF)
- 支持模拟量设定 (数值)

### 📊 实时功能
- 实时数据点状态更新
- 协议日志显示 (TX/RX/SIM)
- 统计信息 (发送/接收/错误计数)
- 物理量仿真 (电压/电流/功率/频率)

## 📖 使用指南

### 模拟器模式（Outstation）

1. **选择配置**
   - 模式：**Outstation (Simulator)**
   - 连接类型：**TCP Server**
   - IP地址：`127.0.0.1`
   - 端口：`20000`

2. **启动模拟器**
   - 点击 **RUN** 按钮
   - 观察数据点开始模拟变化
   - 查看实时日志

3. **接收控制命令**
   - 模拟器自动响应Master的控制命令
   - 日志显示SELECT和OPERATE操作

### 主站模式（Master）

1. **连接配置**
   - 模式：**Master (Debugger)**
   - 连接类型：**TCP Client**
   - IP地址：`127.0.0.1` (连接到Outstation)
   - 端口：`20000`

2. **连接到设备**
   - 点击 **CONNECT** 按钮
   - 等待连接成功

3. **发送控制命令**
   - 在数据点表格中点击 **Control** 或 **Set** 按钮
   - 选择操作模式：
     - **Direct Operate** - 立即执行
     - **Select Before Operate** - 两步操作
   - 输入控制值：
     - 二进制：`ON` / `OFF` / `1` / `0`
     - 模拟量：数字（如 `50.5`）
   - 点击 **SEND** 发送命令

## 🔧 配置文件

### 加载配置
1. 点击 **LOAD** 按钮
2. 选择JSON配置文件
3. 数据点自动加载

### 保存配置
1. 点击 **SAVE** 按钮
2. 下载当前配置为JSON文件

### 配置示例
```json
{
  "name": "My DNP3 Device",
  "binary_inputs": [
    {"index": 0, "name": "Breaker Status"}
  ],
  "binary_outputs": [
    {"index": 0, "name": "Breaker Control"}
  ],
  "analog_inputs": [
    {"index": 0, "name": "Voltage A"},
    {"index": 1, "name": "Current A"}
  ],
  "analog_outputs": [
    {"index": 0, "name": "Setpoint"}
  ],
  "counters": [
    {"index": 0, "name": "Energy Counter"}
  ]
}
```

## 📋 协议日志

日志类型：
- **[TX]** - 发送的命令（绿色）
- **[RX]** - 接收的响应（蓝色）
- **[SIM]** - 模拟事件（黄色）
- **[System]** - 系统消息（黄色）
- **[Error]** - 错误信息（红色）

日志示例：
```
[14:23:45] [TX] Direct - BinaryOutput[0] = 1.0
[14:23:45] [RX] SUCCESS - BinaryOutput[0] updated to 1.0
[14:23:46] [SIM] AI[0] 230.12 → 232.45
[14:23:47] [SIM] BI[2] = ON
```

## 🏗️ 技术栈

- **后端：** Rust + Tokio + Axum
- **前端：** HTML5 + CSS3 + Vanilla JavaScript
- **协议：** DNP3 (IEEE 1815-2012)
- **部署：** 单文件可执行程序

## 📦 项目结构

```
dnp3-tester/
├── src/
│   ├── main.rs           # Web服务器和API端点
│   ├── dnp3_service.rs   # DNP3核心服务
│   └── models.rs         # 数据模型
├── frontend/
│   ├── index.html        # 用户界面
│   ├── app.js           # 前端逻辑
│   ├── styles.css       # 样式表
│   └── default_config.json  # 默认配置
├── Cargo.toml           # Rust依赖
└── STATUS.md           # 详细状态报告
```

## 🎯 当前状态

- ✅ **完整的UI和交互功能**
- ✅ **数据点实时模拟**
- ✅ **控制操作执行**
- ✅ **协议日志显示**
- ✅ **SBO/DBO模式支持**
- ⚠️ **模拟模式**（真实DNP3集成进行中）

查看 [STATUS.md](STATUS.md) 了解详细的实施状态和路线图。

## 🔍 功能演示

### 物理量模拟
应用自动模拟真实的电气参数：
- **电压：** 230V ± 2V + 5V正弦波
- **电流：** 100A ± 5A + 40A余弦负载变化
- **频率：** 50Hz ± 0.05Hz
- **功率因数：** 0.95 ± 0.04

### 控制响应
- 二进制控制：即时ON/OFF切换
- 模拟量设定：精确数值控制
- 操作确认：TX → RX日志链
- 错误处理：无效索引检测

## 📞 支持

如有问题或建议，请查看：
- [STATUS.md](STATUS.md) - 详细状态报告
- [tasks.md](tasks.md) - 开发任务

## 📄 许可证

MIT License

---

**😊 Big GiantBaby 👍**

*IEEE 1815-2012 DNP3 Protocol Tester* | *v1.0.0*

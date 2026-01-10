// app.js - DNP3 Tester (Restored & Fixed)

// --- Session Logic ---
const sessionId = sessionStorage.getItem('dnp3_session_id') || Math.random().toString(36).substring(7);
sessionStorage.setItem('dnp3_session_id', sessionId);
const originalFetch = window.fetch;
window.fetch = function (url, options = {}) {
    options.headers = options.headers || {};
    options.headers['X-Session-ID'] = sessionId;
    return originalFetch(url, options);
};

// --- Globals ---
let currentConfig = null;
let isRunning = false;
let pollInterval = null;
let logViewMode = 'logs'; // 'logs' or 'frames'
let logsCursor = -1;
let framesCursor = -1;

// --- Initialization ---
document.addEventListener('DOMContentLoaded', () => {
    // Only verify setup if elements exist to avoid null errors
    if (document.getElementById('actionBtn')) {
        setupEventListeners();
        if (typeof loadDefaultConfig === 'function') loadDefaultConfig();
        // Try to detect host LAN IP from server and populate IP field (non-invasive)
        if (typeof loadHostIp === 'function') loadHostIp();
        updateModeUI();
        // Attempt serial discovery early so UI shows available ports
        fetchSerialPorts();
    }
});

// Fetch host IP detected by the server (best-effort) and prefill IP field when empty
async function loadHostIp() {
    try {
        const resp = await fetch('/api/host_ip');
        if (!resp.ok) return;
        const j = await resp.json();
        if (j && j.ip) {
            const ipInput = document.getElementById('ipAddress');
            if (ipInput && (!ipInput.value || ipInput.value.trim() === '')) {
                ipInput.value = j.ip;
            }
        }
    } catch (e) {
        // ignore failures - detection is optional
    }
}

function setupEventListeners() {
    document.getElementById('loadBtn').addEventListener('click', () => document.getElementById('configFileInput').click());
    document.getElementById('configFileInput').addEventListener('change', handleFileLoad);
    document.getElementById('saveBtn').addEventListener('click', saveConfiguration);
    document.getElementById('addPointBtn').addEventListener('click', addPoint);
    document.getElementById('clearPointsBtn').addEventListener('click', clearPoints);
    document.getElementById('actionBtn').addEventListener('click', toggleConnection);
    document.getElementById('readBtn').addEventListener('click', manualRead);
    document.getElementById('autoPolling').addEventListener('change', toggleAutoPolling);
    document.getElementById('modeSelect').addEventListener('change', updateModeUI);
    document.getElementById('connType').addEventListener('change', updateConnectionFields);
}

// --- Serial Port Discovery & Helpers ---
async function fetchSerialPorts() {
    const sel = document.getElementById('serialName');
    if (!sel) return;
    // show a temporary option
    sel.innerHTML = '<option value="">-- Detecting ports... --</option>';
    try {
        const resp = await fetch('/api/serial_ports');
        if (!resp.ok) {
            sel.innerHTML = '<option value="">-- No ports detected --</option>';
            return;
        }
        const j = await resp.json();
        const ports = j.ports || [];
        sel.innerHTML = '';
        if (ports.length === 0) {
            sel.innerHTML = '<option value="">-- No serial ports --</option>';
            return;
        }
                // Reorder ports: USB devices first, then non-Bluetooth cu.*, then others, Bluetooth last
                const usbPorts = [];
                const nonBtCu = [];
                const btPorts = [];
                const others = [];
                for (const p of ports) {
                    const lower = p.toLowerCase();
                    if (lower.includes('usbserial') || lower.includes('tty.usb') || /ttyusb/i.test(p) || /usb/i.test(lower)) {
                        usbPorts.push(p);
                    } else if (p.includes('cu.') && !p.includes('BLTH') && !lower.includes('bluetooth')) {
                        nonBtCu.push(p);
                    } else if (p.includes('BLTH') || lower.includes('bluetooth')) {
                        btPorts.push(p);
                    } else {
                        others.push(p);
                    }
                }

                const ordered = [].concat(usbPorts, nonBtCu, others, btPorts);

                // Populate select in preferred order and auto-select the first USB (if any)
                for (const p of ordered) {
                    const opt = document.createElement('option');
                    opt.value = p;
                    opt.text = p;
                    sel.appendChild(opt);
                }

                if (ordered.length > 0) {
                    // If a USB port exists, it will be at index 0 due to ordering
                    sel.value = ordered[0];
                }
    } catch (e) {
        sel.innerHTML = '<option value="">-- Error detecting ports --</option>';
        console.error('Serial discovery failed', e);
    }
}

// --- UI Logic ---
function updateConnectionFields() {
    const type = document.getElementById('connType').value;
    const tcp = document.getElementById('tcpFields');
    const serial = document.getElementById('serialFields');
    if (type === 'serial') {
        tcp.style.display = 'none';
        serial.style.display = 'block';
        // Only populate serial port list when serial selected and not running
        // to avoid changing user's selected device after connection
        if (!isRunning) {
            fetchSerialPorts();
        }
    } else {
        tcp.style.display = 'block';
        serial.style.display = 'none';
    }
}

function updateModeUI() {
    const mode = document.getElementById('modeSelect').value;
    const btn = document.getElementById('actionBtn');
    if (isRunning) {
        if (mode === 'outstation') btn.innerText = "STOP";
        else btn.innerText = "DISCONNECT";
        btn.className = "btn-danger";
    } else {
        if (mode === 'outstation') {
            btn.innerText = "RUN";
            if (document.getElementById('connType').value === 'tcp_client') {
                document.getElementById('connType').value = 'tcp_server';
            }
            document.getElementById('localAddr').value = '10';
            document.getElementById('remoteAddr').value = '1';
        } else {
            btn.innerText = "CONNECT";
            if (document.getElementById('connType').value === 'tcp_server') {
                document.getElementById('connType').value = 'tcp_client';
            }
            document.getElementById('localAddr').value = '1';
            document.getElementById('remoteAddr').value = '10';
        }
        btn.className = "btn-primary";
    }
    updateConnectionFields();
    refreshIpPortLabels();
}

// Update labels for IP/Port depending on selected mode
function refreshIpPortLabels() {
    const mode = document.getElementById('modeSelect').value;
    const ipLabel = document.getElementById('ipLabel');
    const portLabel = document.getElementById('portLabel');
    const ipInput = document.getElementById('ipAddress');
    const portInput = document.getElementById('port');

    if (!ipLabel || !portLabel) return;

    if (mode === 'master') {
        ipLabel.innerText = 'Remote IP (Outstation)';
        portLabel.innerText = 'Remote Port';
        ipInput.placeholder = 'e.g. 192.168.0.2';
        portInput.placeholder = 'e.g. 20000';
    } else {
        ipLabel.innerText = 'Bind IP (This host)';
        portLabel.innerText = 'Port';
        ipInput.placeholder = 'Leave empty to bind 0.0.0.0';
        portInput.placeholder = 'e.g. 20000';
    }
}

// --- Connection Logic ---
async function toggleConnection() {
    if (isRunning) await disconnectMaster();
    else await connectMaster();
    updateModeUI();
    updateMasterControls();
}

async function connectMaster() {
    if (currentConfig) {
        try {
            await fetch('/api/config/apply', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(currentConfig)
            });
        } catch (e) {
            addLog("Error", "Failed to apply config.");
            return;
        }
    } else {
        alert("Please Load Configuration first.");
        return;
    }

    const config = {
        mode: document.getElementById('modeSelect').value,
        connType: document.getElementById('connType').value,
        ip: document.getElementById('ipAddress').value,
        port: parseInt(document.getElementById('port').value),
        serialName: document.getElementById('serialName').value,
        baudRate: parseInt(document.getElementById('baudRate').value),
        dataBits: parseInt(document.getElementById('dataBits').value),
        parity: document.getElementById('parity').value,
        stopBits: parseInt(document.getElementById('stopBits').value),
        timeout: parseInt(document.getElementById('linkTimeout').value),
        localAddr: parseInt(document.getElementById('localAddr').value),
        remoteAddr: parseInt(document.getElementById('remoteAddr').value)
    };

    // If serial selected, ensure a serial device is chosen
    if (config.connType === 'serial' && (!config.serialName || config.serialName.trim() === '')) {
        alert('Please select a physical serial device before connecting.');
        return;
    }

    // Validate / normalize IP to avoid empty strings which can cause server bind errors
    if (!config.ip || config.ip.trim() === '') {
        if (config.mode === 'outstation') {
            // Bind to all interfaces by default for outstation
            config.ip = '0.0.0.0';
        } else {
            // Default master target to localhost
            // Leave empty - normalization will bind to 0.0.0.0 for Outstation
            // and master will default to localhost when needed.
        }
    }

    addLog("System", `Connecting...`);
    try {
        const response = await fetch('/api/connect', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(config) });
        const res = await response.json();
        if (res.success) {
            isRunning = true;
            addLog('System', 'Service started. Connecting...');
            document.getElementById('connStatus').innerText = "RUNNING";
            document.getElementById('connStatus').className = "status-connected";
            const mode = document.getElementById('modeSelect').value;
            if (mode === 'master') {
                const autoPolling = document.getElementById('autoPolling').checked;
                if (autoPolling) pollInterval = setInterval(fetchDataPoints, 5000);
            } else {
                pollInterval = setInterval(fetchDataPoints, 1000); // 1s for Outstation
            }
        } else { addLog("Error", `Connection failed: ${res.error}`); }
    } catch (e) { addLog("Error", `Connection error: ${e.message}`); }
}

async function disconnectMaster() {
    try {
        await fetch('/api/disconnect', { method: 'POST' });
        addLog("System", "Disconnected.");
        isRunning = false;
        clearInterval(pollInterval);
        document.getElementById('connStatus').innerText = "DISCONNECTED";
        document.getElementById('connStatus').className = "status-disconnected";
        if (currentConfig && typeof renderConfigTable === 'function') renderConfigTable(currentConfig);
    } catch (e) { console.error(e); }
}

// --- Polling & Master Controls ---
async function manualRead() {
    if (!isRunning) return;
    addLog("System", "Manual read triggered");
    await fetchDataPoints();
}

function toggleAutoPolling() {
    const enabled = document.getElementById('autoPolling').checked;
    if (enabled) {
        addLog("System", "Auto-polling enabled");
        if (!pollInterval && isRunning) pollInterval = setInterval(fetchDataPoints, 5000);
    } else {
        addLog("System", "Auto-polling disabled");
        if (pollInterval) { clearInterval(pollInterval); pollInterval = null; }
    }
}

function updateMasterControls() {
    const mode = document.getElementById('modeSelect').value;
    const controls = document.getElementById('masterControls');
    controls.style.display = (mode === 'master' && isRunning) ? 'block' : 'none';
}

async function fetchDataPoints() {
    if (!isRunning) return;
    const mode = document.getElementById('modeSelect').value;
    if (mode === 'master') {
        try { await fetch('/api/read', { method: 'POST' }); } catch (e) { }
    }

    try {
        const response = await fetch('/api/data');
        if (!response.ok) return;
        const data = await response.json();

        if (data.stats) {
            document.getElementById('statTx').innerText = data.stats.tx || 0;
            document.getElementById('statRx').innerText = data.stats.rx || 0;
            document.getElementById('statErr').innerText = data.stats.errors || 0;
        }

        if (typeof renderLiveTable === 'function') renderLiveTable(data.points);

        // Incremental Log/Frame Update
        fetchLogs();
        fetchFrames();
    } catch (e) { console.error(e); }
}

// --- Consolidated Logging Logic (Newest on Top) ---

function toggleLogView() {
    const btn = document.getElementById('logViewToggle');
    const logsView = document.getElementById('protocolLog');
    const framesView = document.getElementById('protocolFrames');
    if (logViewMode === 'logs') {
        logViewMode = 'frames';
        btn.innerHTML = '📦 FRAMES';
        btn.className = 'btn btn-sm btn-success';
        logsView.style.display = 'none';
        framesView.style.display = 'block';
        fetchFrames();
    } else {
        logViewMode = 'logs';
        btn.innerHTML = '📋 LOGS';
        btn.className = 'btn btn-sm btn-primary';
        logsView.style.display = 'block';
        framesView.style.display = 'none';
        fetchLogs();
    }
}

// Fetch Logs with Cursor (Newest Logic)
function fetchLogs() {
    if (logViewMode !== 'logs') return;
    fetch('/api/logs').then(r => r.json()).then(data => {
        const container = document.getElementById('protocolLog');
        const newLogs = data.logs.filter(l => l.id > logsCursor);
        if (newLogs.length === 0) return;
        logsCursor = Math.max(logsCursor, ...newLogs.map(l => l.id));
        newLogs.forEach(log => {
            const div = document.createElement('div');
            div.className = 'log-entry';
            let color = '#ccc';
            if (log.direction === 'TX') color = '#4caf50';
            if (log.direction === 'RX') color = '#2196f3';
            if (log.direction === 'Error') color = '#f44336';
            if (log.direction === 'System') color = '#ffeb3b';
            const time = new Date(log.timestamp).toLocaleTimeString();
            div.innerHTML = `<span class="log-time" style="color: grey;">[${time}]</span> <span class="log-msg"><strong style="color: ${color};">[${log.direction}]</strong> ${log.message}</span>`;
            container.prepend(div);
        });
        while (container.children.length > 500) container.removeChild(container.lastChild);
    }).catch(console.error);
}

// Fetch Frames with Cursor (Newest Logic)
function fetchFrames() {
    if (logViewMode !== 'frames') return;
    fetch('/api/frames').then(r => r.json()).then(data => {
        const container = document.getElementById('protocolFrames');
        if (container.querySelector('div[style*="text-align: center"]')) container.innerHTML = '';
        const frames = data.frames || [];
        const newFrames = frames.filter(f => f.id > framesCursor);
        if (newFrames.length === 0) return;
        framesCursor = Math.max(framesCursor, ...newFrames.map(f => f.id));

        newFrames.forEach((frame) => {
            const div = document.createElement('div');
            const direction = frame.direction.toLowerCase();
            const directionIcon = direction === 'tx' ? '📤' : '📥';
            const time = new Date(frame.timestamp).toLocaleTimeString();

            // Set CSS classes instead of inline styles
            div.className = `frame-item ${direction}`;

            // Format hex dump and parse DNP3
            let hexLines = 'No Data';
            if (typeof formatHexDump === 'function') hexLines = formatHexDump(frame.data);
            let dnp3Info = '';
            if (typeof parseDNP3Frame === 'function') dnp3Info = parseDNP3Frame(frame.data);

            // Build HTML with CSS classes
            div.innerHTML = `
                <div class="frame-header">
                    <span class="frame-title ${direction}">${directionIcon} Frame #${frame.id} ${frame.direction}</span>
                    <span class="frame-time">${time}</span>
                </div>
                ${dnp3Info ? `
                <div class="frame-dnp3-section">
                    <div class="frame-dnp3-title">🔍 DNP3 Structure:</div>
                    <div class="frame-dnp3-content">${dnp3Info}</div>
                </div>
                ` : ''}
                <div class="frame-hex-section">
                    <div class="frame-hex-title">📦 Raw Hex (${frame.data.length} bytes):</div>
                    <div class="frame-hex-content">${hexLines}</div>
                </div>
            `;

            container.prepend(div);
        });
        while (container.children.length > 200) container.removeChild(container.lastChild);
    }).catch(console.error);
}

function clearLogs() {
    if (logViewMode === 'logs') document.getElementById('protocolLog').innerHTML = '';
    else document.getElementById('protocolFrames').innerHTML = '';
}

function addLog(type, msg) {
    const container = document.getElementById('protocolLog');
    if (!container) return;
    const div = document.createElement('div');
    div.className = 'log-entry';
    let color = '#ccc';
    if (type === 'TX') color = '#4caf50';
    if (type === 'RX') color = '#2196f3';
    if (type === 'Error') color = '#f44336';
    if (type === 'System') color = '#ffeb3b';
    const time = new Date().toLocaleTimeString();
    div.innerHTML = `<span class="log-time" style="color: grey;">[${time}]</span> <span class="log-msg"><strong style="color: ${color};">[${type}]</strong> ${msg}</span>`;
    container.prepend(div);
    while (container.children.length > 500) container.removeChild(container.lastChild);
}

// ... Additional helpers (Config, HexDump, Controls) will be appended below ...

// --- Configuration Logic ---
async function loadDefaultConfig() {
    try {
        const response = await fetch('/default_config.json');
        if (!response.ok) throw new Error("Failed");
        const config = await response.json();
        currentConfig = config;
        addLog("System", "Default config loaded.");
        if (!isRunning) renderConfigTable(config);
    } catch (err) { addLog("Error", "Failed to load default config."); }
}

function handleFileLoad(event) {
    const file = event.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (e) => {
        try {
            currentConfig = JSON.parse(e.target.result);
            addLog("System", `Loaded config: ${file.name}`);
            if (!isRunning) renderConfigTable(currentConfig);
        } catch (err) { alert("Invalid JSON"); }
    };
    reader.readAsText(file);
}

function saveConfiguration() {
    if (!currentConfig) return;
    const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(currentConfig, null, 2));
    const dl = document.createElement('a');
    dl.setAttribute("href", dataStr);
    dl.setAttribute("download", "dnp3_config.json");
    document.body.appendChild(dl);
    dl.click();
    dl.remove();
}

// --- Rendering Logic ---

function renderConfigTable(config) {
    const tbody = document.getElementById('dataPointsTable');
    if (!tbody) return;
    tbody.innerHTML = '';
    const groups = [
        { data: config.binary_inputs, type: 'BinaryInput' },
        { data: config.binary_outputs, type: 'BinaryOutput' },
        { data: config.analog_inputs, type: 'AnalogInput' },
        { data: config.analog_outputs, type: 'AnalogOutput' },
        { data: config.counters, type: 'Counter' }
    ];
    groups.forEach(g => {
        if (!g.data) return;
        g.data.forEach(point => {
            const tr = document.createElement('tr');
            tr.innerHTML = `<td>${g.type}</td><td>${point.index}</td><td>${point.name}</td><td><span class="badge badge-neutral">-</span></td><td>OFFLINE</td><td>-</td><td><button class="btn btn-sm btn-secondary" onclick="editPoint('${g.type}', ${point.index})">Edit</button></td>`;
            tbody.appendChild(tr);
        });
    });
}

function renderLiveTable(points) {
    const tbody = document.getElementById('dataPointsTable');
    if (!tbody) return;
    tbody.innerHTML = '';
    points.forEach(point => {
        const tr = document.createElement('tr');
        let valClass = 'badge-neutral';
        let displayValue = point.value;
        let actionBtn = '';

        if (point.type.includes('Binary')) {
            const boolVal = (point.value > 0.5);
            displayValue = boolVal ? 'ON' : 'OFF';
            valClass = boolVal ? 'badge-success' : 'badge-danger';
            if (point.type.includes('Output')) actionBtn = `<button class="btn btn-sm btn-primary" onclick="openControl(${point.index}, '${point.type}')">Control</button>`;
        } else if (point.type.includes('Analog')) {
            displayValue = parseFloat(point.value).toFixed(2);
            if (point.name.toLowerCase().includes('voltage')) displayValue += ' V';
            else if (point.name.toLowerCase().includes('current')) displayValue += ' A';
            valClass = 'badge-info';
            if (point.type.includes('Output')) actionBtn = `<button class="btn btn-sm btn-primary" onclick="openControl(${point.index}, '${point.type}')">Set</button>`;
        } else if (point.type === 'Counter') {
            displayValue = parseInt(point.value);
            valClass = 'badge-primary';
        }

        tr.innerHTML = `<td>${point.type}</td><td>${point.index}</td><td>${point.name}</td><td><span class="badge ${valClass}">${displayValue}</span></td><td>${point.quality || 'ONLINE'}</td><td>${point.timestamp ? new Date(point.timestamp).toLocaleTimeString() : ''}</td><td>${actionBtn}</td>`;
        tbody.appendChild(tr);
    });
}

window.editPoint = function (type, index) {
    if (isRunning) return;
    let key = null;
    if (type === 'BinaryInput') key = 'binary_inputs';
    else if (type === 'BinaryOutput') key = 'binary_outputs';
    else if (type === 'AnalogInput') key = 'analog_inputs';
    else if (type === 'AnalogOutput') key = 'analog_outputs';
    else if (type === 'Counter') key = 'counters';

    if (!key || !currentConfig[key]) return;
    const point = currentConfig[key].find(p => p.index === index);
    if (!point) return;

    const newName = prompt(`Edit Name for ${type} [${index}]:`, point.name);
    if (newName !== null && newName.trim() !== "") {
        point.name = newName;
        renderConfigTable(currentConfig);
    }
};

// --- Data Point Management ---

async function addPoint() {
    if (!isRunning) {
        alert("Please connect to a device first");
        return;
    }

    // 1. Select point type
    const pointType = prompt(
        "Enter point type:\n" +
        "1 = BinaryInput\n" +
        "2 = BinaryOutput\n" +
        "3 = AnalogInput\n" +
        "4 = AnalogOutput\n" +
        "5 = Counter",
        "1"
    );

    if (!pointType) return; // User cancelled

    const typeMap = {
        '1': 'BinaryInput',
        '2': 'BinaryOutput',
        '3': 'AnalogInput',
        '4': 'AnalogOutput',
        '5': 'Counter'
    };

    const selectedType = typeMap[pointType];
    if (!selectedType) {
        alert("Invalid point type. Must be 1-5");
        return;
    }

    // 2. Enter index
    const indexStr = prompt(`Enter index for ${selectedType}:`, "0");
    if (!indexStr) return;

    const index = parseInt(indexStr);
    if (isNaN(index) || index < 0 || index > 65535) {
        alert("Invalid index. Must be 0-65535");
        return;
    }

    // 3. Enter name
    const name = prompt(`Enter name for ${selectedType}[${index}]:`, `Point_${index}`);
    if (!name) return;

    // Send to backend
    try {
        const response = await fetch('/api/datapoints/add', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'X-Session-ID': sessionId
            },
            body: JSON.stringify({
                point_type: selectedType,
                index: index,
                name: name
            })
        });

        const result = await response.json();

        if (result.success) {
            addLog("System", `✓ Added ${selectedType}[${index}] - ${name}`);
            // Refresh data immediately
            fetchDataPoints();
        } else {
            alert(`Failed to add point: ${result.error || 'Unknown error'}`);
            addLog("Error", `✗ Add point failed: ${result.error}`);
        }
    } catch (e) {
        alert(`Network error: ${e.message}`);
        addLog("Error", `✗ Network error: ${e.message}`);
    }
}

async function clearPoints() {
    if (!isRunning) {
        alert("Please connect to a device first");
        return;
    }

    const confirmed = confirm(
        "⚠️ Are you sure you want to CLEAR ALL data points?\n\n" +
        "This will remove all points from the current session.\n" +
        "This action cannot be undone."
    );

    if (!confirmed) return;

    try {
        const response = await fetch('/api/datapoints/clear', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'X-Session-ID': sessionId
            }
        });

        const result = await response.json();

        if (result.success) {
            addLog("System", "🗑️ All data points cleared");
            // Refresh data immediately
            fetchDataPoints();
        } else {
            alert(`Failed to clear points: ${result.error || 'Unknown error'}`);
            addLog("Error", `✗ Clear failed: ${result.error}`);
        }
    } catch (e) {
        alert(`Network error: ${e.message}`);
        addLog("Error", `✗ Network error: ${e.message}`);
    }
}

// --- Helpers (Hex, Parsing) ---

function formatHexDump(bytes) {
    if (!bytes || bytes.length === 0) return 'No data';
    let lines = [];
    for (let i = 0; i < bytes.length; i += 16) {
        const chunk = bytes.slice(i, i + 16);
        const offset = i.toString(16).padStart(4, '0').toUpperCase();
        const hex = chunk.map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ');
        const ascii = chunk.map(b => (b >= 32 && b < 127) ? String.fromCharCode(b) : '.').join('');
        lines.push(`<span style="color: #888;">${offset}:</span>  ${hex.padEnd(48, ' ')}  <span style="color: #666;">|${ascii}|</span>`);
    }
    return lines.join('\n');
}

function parseDNP3Frame(bytes) {
    if (!bytes || bytes.length < 10) return null;
    try {
        if (bytes[0] === 0x05 && bytes[1] === 0x64) {
            const len = bytes[2];
            const ctrl = bytes[3];
            const dest = bytes[4] | (bytes[5] << 8);
            const src = bytes[6] | (bytes[7] << 8);
            const crc = bytes[8] | (bytes[9] << 8);
            let info = `<strong style="color: #ff9800;">LINK:</strong> Start=05 64, Len=${len}, Ctrl=0x${ctrl.toString(16).toUpperCase()}, Dest=${dest}, Src=${src}, CRC=0x${crc.toString(16).toUpperCase()}<br>`;

            if (bytes.length > 10) {
                const transport = bytes[10];
                const fir = (transport & 0x40) ? 1 : 0;
                const fin = (transport & 0x80) ? 1 : 0;
                const seq = transport & 0x3F;
                info += `<strong style="color: #9c27b0;">TRANS:</strong> FIR=${fir} FIN=${fin} SEQ=${seq}<br>`;

                if (bytes.length > 11) {
                    const appCtrl = bytes[11];
                    const fc = bytes[12];
                    const fcName = getFunctionCodeName(fc.toString(16).padStart(2, '0'));
                    info += `<strong style="color: #03a9f4;">APP:</strong> AC=0x${appCtrl.toString(16).toUpperCase()} FC=0x${fc.toString(16).toUpperCase()} (${fcName})`;
                }
            }
            return info;
        }
    } catch (e) { return null; }
    return null;
}

function getFunctionCodeName(fc) {
    const codes = { '01': 'READ', '03': 'SELECT', '04': 'OPERATE', '05': 'DIRECT OPERATE', '81': 'RESPONSE', '82': 'UNSOLICITED RESPONSE' };
    return codes[fc] || 'UNKNOWN';
}

// --- Control Modal Logic ---

let currentControlPoint = null;
let sboSelectState = null; // Track SBO selection state

window.openControl = function (index, type) {
    currentControlPoint = { index, type };
    sboSelectState = null; // Reset SBO state

    document.getElementById('controlTitle').innerText = `Control ${type} [Index ${index}]`;
    document.getElementById('controlValue').value = '';

    const modeSelect = document.getElementById('controlMode');
    modeSelect.innerHTML = `<option value="DirectOperate">Direct Operate (0x05)</option><option value="DirectOperateNoAck">Direct Operate No Ack (0x06)</option><option value="SelectBeforeOperate">Select Before Operate (SBO 0x03+0x04)</option>`;

    // Set default to Direct Operate and update buttons
    modeSelect.value = 'DirectOperate';
    updateControlButtons();

    document.getElementById('controlModal').style.display = 'flex';
};

window.closeControlModal = function () {
    document.getElementById('controlModal').style.display = 'none';
    currentControlPoint = null;
    sboSelectState = null;
};

// Update button visibility based on operation mode
window.updateControlButtons = function () {
    const mode = document.getElementById('controlMode').value;
    const directButtons = document.getElementById('directOperateButtons');
    const sboButtons = document.getElementById('sboButtons');

    if (mode === 'SelectBeforeOperate') {
        // SBO mode: Show Select/Operate/Cancel buttons
        directButtons.style.display = 'none';
        sboButtons.style.display = 'flex';

        // Reset SBO state when switching to SBO mode
        sboSelectState = null;
        updateSBOButtonStates();
    } else {
        // Direct Operate modes (0x05/0x06): Show single Operate button
        directButtons.style.display = 'flex';
        sboButtons.style.display = 'none';
    }
};

// Update SBO button states based on selection status
function updateSBOButtonStates() {
    const selectBtn = document.querySelector('#sboButtons .btn-info');
    const operateBtn = document.querySelector('#sboButtons .btn-success');

    console.log('Updating SBO button states, sboSelectState:', sboSelectState);

    if (!selectBtn || !operateBtn) {
        console.warn('SBO buttons not found!');
        return;
    }

    if (sboSelectState === 'selected') {
        // After Select: Enable Operate, disable Select
        console.log('Enabling Operate, disabling Select');
        selectBtn.disabled = true;
        operateBtn.disabled = false;
    } else {
        // Initial state: Enable Select, disable Operate
        console.log('Enabling Select, disabling Operate');
        selectBtn.disabled = false;
        operateBtn.disabled = true;
    }
}

// Direct Operate (0x05 or 0x06)
window.submitDirectOperate = async function () {
    if (!currentControlPoint) return;

    const valueStr = document.getElementById('controlValue').value;
    if (!valueStr) {
        alert("Please enter a value");
        return;
    }

    const modeSelect = document.getElementById('controlMode').value;

    // Map frontend mode to backend op_mode
    let mode = 'Direct';  // Default: 0x05
    if (modeSelect === 'DirectOperateNoAck') mode = 'DirectNoAck'; // 0x06

    let value = parseFloat(valueStr);
    if (currentControlPoint.type.includes('Binary')) {
        if (valueStr.toLowerCase() === 'on' || valueStr.toLowerCase() === 'true' || valueStr === '1') value = 1.0;
        else value = 0.0;
    }

    try {
        const response = await fetch('/api/control', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                point_type: currentControlPoint.type,
                index: parseInt(currentControlPoint.index),
                value: value,
                op_mode: mode  // "Direct" or "DirectNoAck"
            })
        });
        const res = await response.json();
        if (res.status === 'success') {
            addLog("System", `✓ ${mode} (FC ${mode === 'DirectNoAck' ? '0x06' : '0x05'}) executed`);
            closeControlModal();
        } else {
            addLog("Error", `✗ Control Failed: ${res.message}`);
        }
    } catch (e) {
        addLog("Error", `✗ Network: ${e.message}`);
    }
};

// SBO: Step 1 - Select (FC 0x03) - Real API call
window.submitSelect = async function () {
    if (!currentControlPoint) return;

    console.log('submitSelect called');

    const valueStr = document.getElementById('controlValue').value;
    if (!valueStr) {
        alert("Please enter a value first");
        return;
    }

    let value = parseFloat(valueStr);
    if (currentControlPoint.type.includes('Binary')) {
        if (valueStr.toLowerCase() === 'on' || valueStr.toLowerCase() === 'true' || valueStr === '1') value = 1.0;
        else value = 0.0;
    }

    console.log('Sending Select command with value:', value);

    try {
        // Send actual Select command to backend
        // Note: DNP3 library limitation - this may send both Select + Operate
        // See backend dnp3_service.rs TODO comment
        const response = await fetch('/api/control', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                point_type: currentControlPoint.type,
                index: parseInt(currentControlPoint.index),
                value: value,
                op_mode: 'Select'  // Backend will attempt FC 0x03
            })
        });
        const res = await response.json();
        console.log('Select response:', res);

        if (res.status === 'success') {
            console.log('Select successful, setting sboSelectState to selected');
            sboSelectState = 'selected';
            updateSBOButtonStates();
            addLog("System", `✓ Select (FC 0x03) sent - Ready to Operate or Cancel`);
        } else {
            console.error('Select failed:', res.message);
            addLog("Error", `✗ Select Failed: ${res.message}`);
        }
    } catch (e) {
        console.error('Select network error:', e);
        addLog("Error", `✗ Network: ${e.message}`);
    }
};

// SBO: Step 2 - Operate (FC 0x04)
window.submitOperate = async function () {
    if (!currentControlPoint || sboSelectState !== 'selected') {
        addLog("Error", "✗ Must Select before Operate");
        return;
    }

    const valueStr = document.getElementById('controlValue').value;
    let value = parseFloat(valueStr);
    if (currentControlPoint.type.includes('Binary')) {
        if (valueStr.toLowerCase() === 'on' || valueStr.toLowerCase() === 'true' || valueStr === '1') value = 1.0;
        else value = 0.0;
    }

    try {
        // Send Operate command (should follow previous Select)
        const response = await fetch('/api/control', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                point_type: currentControlPoint.type,
                index: parseInt(currentControlPoint.index),
                value: value,
                op_mode: 'Operate'  // Backend will send FC 0x04
            })
        });
        const res = await response.json();
        if (res.status === 'success') {
            addLog("System", `✓ Operate (FC 0x04) sent - SBO Complete`);
            closeControlModal();
        } else {
            addLog("Error", `✗ Operate Failed: ${res.message}`);
            sboSelectState = null;
            updateSBOButtonStates();
        }
    } catch (e) {
        addLog("Error", `✗ Network: ${e.message}`);
        sboSelectState = null;
        updateSBOButtonStates();
    }
};

// SBO: Cancel - Reset Select state, allow re-selection
window.submitCancel = function () {
    console.log('Cancel clicked, current state:', sboSelectState);

    if (sboSelectState === 'selected') {
        // Cancel the Select operation
        sboSelectState = null;
        updateSBOButtonStates();
        addLog("System", "⊘ SBO Select cancelled - You can Select again");
    } else {
        // If not selected, just reset state
        sboSelectState = null;
        updateSBOButtonStates();
        addLog("System", "⊘ SBO state reset");
    }

    // Do NOT close the modal - user can select again
};

<script lang="ts">
    import { SerialPort } from 'tauri-plugin-serialplugin';
    import {
        ClearBuffer,
        DataBits,
        FlowControl,
        Parity,
        StopBits
    } from "../../../../guest-js";

    // Accept port and disconnect callback
    let { portName, onDisconnect } = $props();

    // ========================
    // = Declare $state(...)
    // ========================
    let serialport: SerialPort | undefined = $state(undefined);

    // Connection flag
    let isConnected = $state(false);

    // === Parameters ===
    let baudRate = $state(9600);
    let selectedDataBits = $state(DataBits.Eight);
    let selectedFlowControl = $state(FlowControl.None);
    let selectedParity = $state(Parity.None);
    let selectedStopBits = $state(StopBits.One);
    let timeout = $state(1000);

    // === Data and status ===
    let message = $state('');
    let receivedData = $state('');
    let receivedDataBase64 = $state('');
    let bytesToRead = $state(0);
    let bytesToWrite = $state(0);

    // === Signals ===
    let rtsState = $state(false);
    let dtrState = $state(false);
    let ctsState = $state(false);
    let dsrState = $state(false);
    let riState  = $state(false);
    let cdState  = $state(false);

    // ========================
    // = Lists for <select>   =
    // ========================
    let dataBitsOptions     = $state([DataBits.Five, DataBits.Six, DataBits.Seven, DataBits.Eight]);
    let flowControlOptions  = $state([FlowControl.None, FlowControl.Software, FlowControl.Hardware]);
    let parityOptions       = $state([Parity.None, Parity.Odd, Parity.Even]);
    let stopBitsOptions     = $state([StopBits.One, StopBits.Two]);

    // ------------------------
    // === FUNCTIONS ===
    // ------------------------

    async function connect() {
        try {
            if (!portName) return;

            serialport = new SerialPort({
                path: portName,
                baudRate,
                dataBits: selectedDataBits,
                flowControl: selectedFlowControl,
                parity: selectedParity,
                stopBits: selectedStopBits,
                timeout
            });

            await serialport.open();
            isConnected = true;
            console.log('Connected to port:', portName);

            await serialport.startListening();
            serialport.listen((data) => {
                console.log(`[${portName}] incoming data:`, data);
                receivedData += data;
                updatePortStatus();
            });

            // Event when port is physically disconnected:
            serialport.disconnected(() => {
                isConnected = false;
                console.log(`[${portName}] Disconnected (physically)`);
            });

            updatePortStatus();
        } catch (err) {
            console.error(`Failed to connect to port ${portName}:`, err);
        }
    }

    async function disconnect() {
        try {
            if (serialport) {
                await serialport.close();
                isConnected = false;
                console.log('Disconnected from port:', portName);

                // If parent passed onDisconnect callback — call it
                if (typeof onDisconnect === 'function') {
                    onDisconnect({ port: portName });
                }
            }
        } catch (err) {
            console.error(`Failed to disconnect from port ${portName}:`, err);
        }
    }

    async function sendMessage() {
        if (!serialport || !message) return;
        try {
            await serialport.write(message);
            console.log(`[${portName}] Message sent:`, message);
            message = '';
            updatePortStatus();
        } catch (err) {
            console.error(`Failed to send message on port ${portName}:`, err);
        }
    }

    async function sendBinary() {
        if (!serialport) return;
        try {
            const data = new Uint8Array([1, 2, 3, 4, 5]);
            await serialport.writeBinary(data);
            console.log(`[${portName}] Binary data sent:`, data);
            updatePortStatus();
        } catch (err) {
            console.error(`Failed to send binary data on port ${portName}:`, err);
        }
    }

    async function read() {
        if (!serialport) return;
        try {
            const data = await serialport.read();
            console.log(`[${portName}] Read:`, data);
        } catch (err) {
            console.error(`Failed to read on port ${portName}:`, err);
        }
    }

    async function readBinary() {
        if (!serialport) return;
        try {
            const binaryData = await serialport.readBinary({ size: 3, timeout: 2000 });
            console.log(`[${portName}] Read binary:`, binaryData);
        } catch (err) {
            console.error(`Failed to read binary on port ${portName}:`, err);
        }
    }

    async function updatePortSettings() {
        if (!serialport) return;
        try {
            await serialport.setBaudRate(baudRate);
            await serialport.setDataBits(selectedDataBits);
            await serialport.setFlowControl(selectedFlowControl);
            await serialport.setParity(selectedParity);
            await serialport.setStopBits(selectedStopBits);
            await serialport.setTimeout(timeout);
            console.log(`[${portName}] Port settings updated`);
        } catch (err) {
            console.error(`Failed to update port settings on ${portName}:`, err);
        }
    }

    async function clearBuffers() {
        if (!serialport) return;
        try {
            await serialport.clearBuffer(ClearBuffer.All);
            console.log(`[${portName}] Buffers cleared`);
            updatePortStatus();
        } catch (err) {
            console.error(`Failed to clear buffers on port ${portName}:`, err);
        }
    }

    async function updatePortStatus() {
        if (!serialport) return;
        try {
            bytesToRead = await serialport.bytesToRead();
            bytesToWrite = await serialport.bytesToWrite();
            ctsState = await serialport.readClearToSend();
            dsrState = await serialport.readDataSetReady();
            riState = await serialport.readRingIndicator();
            cdState = await serialport.readCarrierDetect();
        } catch (err) {
            console.error(`Failed to update port status for ${portName}:`, err);
        }
    }

    async function toggleRTS() {
        if (!serialport) return;
        try {
            rtsState = !rtsState;
            await serialport.setRequestToSend(rtsState);
        } catch (err) {
            console.error(`Failed to toggle RTS on ${portName}:`, err);
        }
    }

    async function toggleDTR() {
        if (!serialport) return;
        try {
            dtrState = !dtrState;
            await serialport.setDataTerminalReady(dtrState);
        } catch (err) {
            console.error(`Failed to toggle DTR on ${portName}:`, err);
        }
    }
</script>

<!-- ======================================= -->
<!--            LAYOUT                      -->
<!-- ======================================= -->
<div class="port-container">
    <h2>Port: {portName}</h2>

    {#if isConnected}
        <p class="connected">Status: CONNECTED</p>
    {:else}
        <p class="disconnected">Status: NOT CONNECTED</p>
    {/if}

    <!-- Connection buttons -->
    <div class="row connect-row">
        <button onclick={connect} disabled={isConnected}>Connect</button>
        <button onclick={disconnect} disabled={!isConnected}>Disconnect</button>
    </div>

    <!-- Settings section -->
    <div class="section settings-panel">
        <h3>Port Settings</h3>
        <div class="settings-grid">
            <label>
                Baud Rate
                <input type="number" bind:value={baudRate} />
            </label>

            <label>
                Data Bits
                <select bind:value={selectedDataBits}>
                    {#each dataBitsOptions as bits}
                        <option value={bits}>{bits}</option>
                    {/each}
                </select>
            </label>

            <label>
                Flow Control
                <select bind:value={selectedFlowControl}>
                    {#each flowControlOptions as flow}
                        <option value={flow}>{flow}</option>
                    {/each}
                </select>
            </label>

            <label>
                Parity
                <select bind:value={selectedParity}>
                    {#each parityOptions as parity}
                        <option value={parity}>{parity}</option>
                    {/each}
                </select>
            </label>

            <label>
                Stop Bits
                <select bind:value={selectedStopBits}>
                    {#each stopBitsOptions as sb}
                        <option value={sb}>{sb}</option>
                    {/each}
                </select>
            </label>

            <label>
                Timeout (ms)
                <input type="number" bind:value={timeout} />
            </label>
        </div>

        <button class="update-btn" onclick={updatePortSettings} disabled={!isConnected}>
            Update Settings
        </button>
    </div>

    <!-- Data transfer section -->
    <div class="section data-transfer">
        <h3>Data Transfer</h3>
        <div class="row">
            <input
                    type="text"
                    placeholder="Enter message..."
                    bind:value={message}
                    disabled={!isConnected}
            />
            <button onclick={sendMessage} disabled={!isConnected || !message}>
                Send Text
            </button>
            <button onclick={sendBinary} disabled={!isConnected}>
                Send Binary
            </button>
        </div>

        <div class="row">
            <button onclick={read} disabled={!isConnected}>Read</button>
            <button onclick={readBinary} disabled={!isConnected}>Read Binary</button>
            <button onclick={clearBuffers} disabled={!isConnected}>Clear Buffers</button>
        </div>

        <div class="status-info">
            <p>Bytes to read: {bytesToRead}</p>
            <p>Bytes to write: {bytesToWrite}</p>
        </div>

        <div class="received-data">
            <h4>Received Data:</h4>
            <pre>{receivedData}</pre>
        </div>
    </div>

    <!-- Control signals section -->
    <div class="section control-signals">
        <h3>Control Signals</h3>
        <div class="row signals-row">
            <button
                    onclick={toggleRTS}
                    disabled={!isConnected}
                    class:rts-active={rtsState}
            >
                RTS: {rtsState ? 'ON' : 'OFF'}
            </button>

            <button
                    onclick={toggleDTR}
                    disabled={!isConnected}
                    class:dtr-active={dtrState}
            >
                DTR: {dtrState ? 'ON' : 'OFF'}
            </button>
        </div>

        <div class="signals-indicators">
            <div class:active={ctsState}>CTS: {ctsState ? 'ON' : 'OFF'}</div>
            <div class:active={dsrState}>DSR: {dsrState ? 'ON' : 'OFF'}</div>
            <div class:active={riState}>RI:  {riState ? 'ON' : 'OFF'}</div>
            <div class:active={cdState}>CD:  {cdState ? 'ON' : 'OFF'}</div>
        </div>
    </div>
</div>

<!-- ======================================= -->
<!--            STYLES                      -->
<!-- ======================================= -->
<style>
    .port-container {
        padding: 10px;
        border-radius: 6px;
        background: #fafafa;
        margin-bottom: 20px;
        border: 1px solid #eee;
    }

    h2 {
        margin-top: 0;
        font-weight: 500;
        font-size: 1.2rem;
    }

    .connected {
        color: green;
        font-weight: bold;
    }
    .disconnected {
        color: #999;
        font-weight: bold;
    }

    /* Common blocks / sections */
    .section {
        background: #fff;
        border: 1px solid #eee;
        border-radius: 6px;
        padding: 10px;
        margin-bottom: 1rem;
    }

    .section h3 {
        margin-top: 0;
        font-size: 1rem;
        margin-bottom: 0.5rem;
    }

    /* Row of buttons or fields */
    .row {
        display: flex;
        gap: 10px;
        margin-bottom: 1rem;
    }

    .connect-row {
        margin-bottom: 20px;
    }

    /* Settings grid */
    .settings-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
        gap: 12px;
        margin-bottom: 10px;
    }

    label {
        display: flex;
        flex-direction: column;
        font-weight: 600;
        font-size: 0.9rem;
        color: #333;
    }

    input, select {
        margin-top: 5px;
        padding: 6px;
        font-size: 0.9rem;
        border: 1px solid #ddd;
        border-radius: 4px;
    }

    .update-btn {
        margin-top: 5px;
    }

    /* Buttons */
    button {
        border: none;
        background: #2196f3;
        color: white;
        padding: 8px 14px;
        border-radius: 4px;
        cursor: pointer;
        font-size: 0.9rem;
    }
    button:hover {
        background: #1976d2;
    }
    button:disabled {
        background: #ccc;
        cursor: not-allowed;
    }

    /* Status information */
    .status-info {
        display: flex;
        gap: 1rem;
        font-family: monospace;
        margin: 10px 0;
    }

    .received-data {
        max-height: 150px;
        overflow-y: auto;
        background: #f8f8f8;
        padding: 10px;
        border-radius: 4px;
        margin-top: 10px;
    }
    pre {
        margin: 0;
    }

    /* Control signals */
    .signals-row {
        display: flex;
        gap: 10px;
        margin-bottom: 10px;
    }
    .signals-indicators {
        display: flex;
        gap: 10px;
    }
    .signals-indicators > div {
        background: #f5f5f5;
        padding: 6px 8px;
        border-radius: 4px;
        font-family: monospace;
    }
    .signals-indicators > div.active {
        background: #4caf50;
        color: white;
    }

    .rts-active,
    .dtr-active {
        background: #4caf50;
    }
</style>

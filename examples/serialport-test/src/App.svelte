<script lang="ts">
  import { onMount } from "svelte";
  import { SerialPort } from "tauri-plugin-serialplugin";
  import SerialPortComponent from "./components/SerialPortComponent.svelte";

  // Списки найденных портов
  let availablePorts: { [key: string]: { type: string } } = {};
  let directPorts: { [key: string]: { type: string } } = {};
  let managedPorts: string[] = [];

  // Список активных подключений
  let activePorts: string[] = [];

  // Поле для ручного ввода пути
  let manualPath: string = "";

  // Сканируем порты
  async function scanPorts() {
    try {
      availablePorts = await SerialPort.available_ports();
      console.log("Available ports:", availablePorts);
    } catch (err) {
      console.error("Failed to scan ports:", err);
    }
  }

  async function scanPortsDirect() {
    try {
      directPorts = await SerialPort.available_ports_direct();
      console.log("Direct ports:", directPorts);
    } catch (err) {
      console.error("Failed to scan ports directly:", err);
    }
  }

  async function showManagedPorts() {
    try {
      managedPorts = await SerialPort.managed_ports();
      console.log("Managed ports:", managedPorts);
    } catch (err) {
      console.error("Failed to get Managed ports:", err);
    }
  }

  // Добавить порт (по нажатию "Connect")
  function addPort(portName: string) {
    if (!portName) return;
    if (!activePorts.includes(portName)) {
      activePorts = [...activePorts, portName];
    }
  }

  // Удалить порт из списка активных (по событию disconnect)
  function removePort(portName: string) {
    activePorts = activePorts.filter((name) => name !== portName);
  }

  // Клик «Connect» для ручного пути
  function addManualPort() {
    const path = manualPath.trim();
    if (path) {
      addPort(path);
      manualPath = "";
    }
  }

  function handleDisconnect({ port }: { port: string }) {
    console.log("handleDisconnect сработал, порт:", port);
    removePort(port);
  }

  // При маунте сканируем сразу всё
  onMount(() => {
    scanPorts();
    scanPortsDirect();
    showManagedPorts();
  });
</script>

<main class="container">
  <h1 class="title">Multi-Port Serial Demo</h1>

  <!-- Ручной ввод пути -->
  <section class="manual-connect">
    <h2>Manual Port Input</h2>
    <div class="row">
      <input
        type="text"
        placeholder="Enter port path (e.g. /dev/ttyACM0)..."
        bind:value={manualPath}
      />
      <button on:click={addManualPort}>+</button>
    </div>
  </section>

  <!-- Секции со списками портов -->
  <section class="scan-section">
    <div class="port-lists">
      <!-- Available Ports -->
      <div class="port-group">
        <h3>
          Available Ports
          <button on:click={scanPorts}>Scan Ports</button>
        </h3>
        {#if Object.keys(availablePorts).length > 0}
          <ul>
            {#each Object.entries(availablePorts) as [portName, info]}
              <li>
                <span>{portName} — {info.type}</span>
                <button on:click={() => addPort(portName)}>+</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p>No ports found</p>
        {/if}
      </div>

      <!-- Direct Ports -->
      <div class="port-group">
        <h3>
          Direct Ports
          <button on:click={scanPortsDirect}>Scan Ports Direct</button>
        </h3>
        {#if Object.keys(directPorts).length > 0}
          <ul>
            {#each Object.entries(directPorts) as [portName, info]}
              <li>
                <span>{portName} — {info.type}</span>
                <button on:click={() => addPort(portName)}>+</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p>No direct ports found</p>
        {/if}
      </div>

      <!-- Managed Ports -->
      <div class="port-group">
        <h3>
          Managed Ports
          <button on:click={showManagedPorts}>Show Managed Ports</button>
        </h3>
        {#if managedPorts.length > 0}
          <ul>
            {#each managedPorts as portName}
              <li>
                <span>{portName}</span>
                <button on:click={() => addPort(portName)}>+</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p>No managed ports found</p>
        {/if}
      </div>
    </div>
  </section>

  <!-- Список активных подключений -->
  <section class="active-ports">
    <h2>Active Connections</h2>
    {#if activePorts.length > 0}
      {#each activePorts as portName}
        <div class="port-wrapper">
          <SerialPortComponent {portName} onDisconnect={handleDisconnect} />
        </div>
      {/each}
    {:else}
      <p>No active connections</p>
    {/if}
  </section>
</main>

<style>
  /* Пример стилистики, вы можете дополнять/менять по вкусу */
  .title {
    color: #fff;
    padding-bottom: 30px;
  }

  main {
    color: #333;
  }

  .container {
    max-width: 1200px;
    margin: 0 auto;
    padding: 20px;
    font-family: sans-serif;
  }

  h1,
  h2,
  h3 {
    margin-bottom: 10px;
    color: #333;
  }

  section {
    margin-bottom: 20px;
    background: #f9f9f9;
    border-radius: 6px;
    padding: 15px;
  }

  /* Ручной ввод */
  .manual-connect {
    margin-bottom: 30px;
  }

  .row {
    display: flex;
    gap: 10px;
  }

  input {
    flex: 1;
    padding: 8px;
    border: 1px solid #ddd;
    border-radius: 4px;
    font-size: 14px;
  }

  button {
    border: none;
    background: #2196f3;
    color: white;
    padding: 8px 14px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 14px;
  }
  button:hover {
    background: #1976d2;
  }
  button:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .port-lists {
    display: flex;
    gap: 20px;
    justify-content: space-between;
    flex-wrap: wrap;
  }

  .port-group {
    flex: 1 1 300px;
    background: #fff;
    border: 1px solid #eee;
    border-radius: 6px;
    padding: 10px;
  }

  .port-group h3 {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 0;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 8px 0;
    gap: 10px;
  }

  .active-ports {
    background: #fff;
  }

  .port-wrapper {
    margin-bottom: 20px;
    border: 1px solid #eee;
    border-radius: 6px;
    padding: 10px;
  }
</style>

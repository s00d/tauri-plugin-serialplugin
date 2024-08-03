<script lang="ts">
  import { SerialPort } from 'tauri-plugin-serialplugin';

  let serialport: SerialPort | undefined = undefined;
  let name: string;

  function openPort() {
    serialport = new SerialPort({ portName: name, baudRate: 9600 });
    serialport
      .open()
      .then((res) => {
        console.log('open serialport', res);
      })
      .catch((err) => {
        console.error(err);
      });
  }

  function closePort() {
    serialport
      .close()
      .then((res) => {
        console.log('close serialport', res);
      })
      .catch((err) => {
        console.error(err);
      });
  }

  function available_ports() {
    SerialPort.available_ports()
      .then((res) => {
        console.log('available_ports: ', res);
      })
      .catch((err) => {
        console.error(err);
      });
  }
</script>

<main class="container">
  <h1>Welcome to Tauri Serial Port Plugin!</h1>

  <p>
    Click on the Tauri, Vite, and Svelte logos to learn more.
  </p>

  <div class="row">
    <button on:click={available_ports}>Scan Ports</button>
  </div>

  <div class="row">
    <button on:click={openPort}>Connect</button>
    <input type="text" placeholder="write your com port here..." bind:value={name} />
    <button on:click={closePort}>Disconnect</button>
  </div>


</main>

<style>
  .logo.vite:hover {
    filter: drop-shadow(0 0 2em #747bff);
  }

  .logo.svelte:hover {
    filter: drop-shadow(0 0 2em #ff3e00);
  }
</style>

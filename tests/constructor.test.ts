import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';

describe('SerialPort Constructor', () => {
  describe('constructor', () => {
    it('should create instance with default values', () => {
      const port = new SerialPort({
        path: '/dev/tty.usbserial',
        baudRate: 9600
      });

      expect(port.isOpen).toBe(false);
      expect(port.encoding).toBe('utf-8');
      expect(port.options.dataBits).toBe(DataBits.Eight);
      expect(port.options.flowControl).toBe(FlowControl.None);
      expect(port.options.parity).toBe(Parity.None);
      expect(port.options.stopBits).toBe(StopBits.One);
      expect(port.options.timeout).toBe(200);
      expect(port.size).toBe(1024);
    });

    it('should create instance with custom values', () => {
      const port = new SerialPort({
        path: '/dev/tty.usbserial',
        baudRate: 115200,
        encoding: 'ascii',
        dataBits: DataBits.Seven,
        flowControl: FlowControl.Hardware,
        parity: Parity.Even,
        stopBits: StopBits.Two,
        timeout: 500,
        size: 2048
      });

      expect(port.options.baudRate).toBe(115200);
      expect(port.encoding).toBe('ascii');
      expect(port.options.dataBits).toBe(DataBits.Seven);
      expect(port.options.flowControl).toBe(FlowControl.Hardware);
      expect(port.options.parity).toBe(Parity.Even);
      expect(port.options.stopBits).toBe(StopBits.Two);
      expect(port.options.timeout).toBe(500);
      expect(port.size).toBe(2048);
    });
  });
}); 

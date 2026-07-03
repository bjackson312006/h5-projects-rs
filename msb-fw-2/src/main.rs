#![no_std]
#![no_main]

use cortex_m::peripheral::SCB;
use cortex_m_rt::{ExceptionFrame, exception};
use defmt::debug;
use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Ipv4Address, Ipv4Cidr, StackResources, tcp::TcpSocket};
use embassy_stm32::Config;
use embassy_stm32::bind_interrupts;
use embassy_stm32::{eth, peripherals, rng, time::Hertz, wdg};
use embassy_time::Timer;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct IrqsEth {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<
        'static,
        eth::Ethernet<
            'static,
            peripherals::ETH,
            eth::GenericPhy<eth::Sma<'static, peripherals::ETH_SMA>>,
        >,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    info!("Initializing project...");

    let mut config = Config::default();
    {
        use embassy_stm32::rcc::mux::*;
        use embassy_stm32::rcc::*;

        config.rcc.hsi48 = Some(Default::default());
        config.rcc.hse = Some(Hse {
            freq: Hertz::mhz(25),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV2,
            mul: PllMul::MUL28,
            divp: Some(PllDiv::DIV2),
            divq: Some(PllDiv::DIV2),
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;

        config.rcc.pll2 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV5,
            mul: PllMul::MUL64,
            divp: Some(PllDiv::DIV5),
            divq: Some(PllDiv::DIV5),
            divr: None,
        });

        config.rcc.mux.lpuart1sel = Lpusartsel::PCLK3;
        config.rcc.mux.uart4sel = Usartsel::PCLK1;

        config.rcc.mux.spi1sel = Spi1sel::PLL2_P;
        config.rcc.mux.spi2sel = Spi2sel::PLL2_P;
        config.rcc.mux.spi3sel = Spi3sel::PLL2_P;

        config.rcc.mux.fdcan12sel = Fdcansel::PLL2_Q;

        config.rcc.voltage_scale = VoltageScale::Scale1;
    }

    let p = embassy_stm32::init(config);

    let mut rng = rng::Rng::new(p.RNG, IrqsEth);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);

    let mac_addr = [0x00, 0x80, 0xE1, 0x00, 0x00, 0x04];

    static PACKETS: StaticCell<eth::PacketQueue<4, 4>> = StaticCell::new();
    let device = eth::Ethernet::new(
        PACKETS.init(eth::PacketQueue::<4, 4>::new()),
        p.ETH,
        IrqsEth,
        p.PA1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PB12,
        p.PB15,
        p.PA5,
        mac_addr,
        p.ETH_SMA,
        p.PA2,
        p.PC1,
    );

    let config_net = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(10, 0, 0, 4), 24),
        dns_servers: heapless::Vec::new(),
        gateway: Some(Ipv4Address::new(10, 0, 0, 1)),
    });

    // Init network stack
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        device,
        config_net,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    // Launch network task
    _spawner.spawn(net_task(runner).unwrap());

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 8192];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    {
        use rust_mqtt::buffer::BumpBuffer;
        use rust_mqtt::client::Client;
        use rust_mqtt::client::options::*;
        use rust_mqtt::config::*;
        use rust_mqtt::types::*;
        let connect_options = ConnectOptions::new()
            .clean_start()
            .session_expiry_interval(SessionExpiryInterval::NeverEnd);

        let mut buffer = [0; 10240];
        let mut buffer = BumpBuffer::new(&mut buffer);

        let mut client = Client::<'_, _, _, 10, 10, 30, 10>::new(&mut buffer);

        client
            .connect(
                socket,
                &connect_options,
                Some(MqttString::from_str("rust-mqtt-demo").unwrap()),
            )
            .await
            .unwrap();

        let topic = TopicName::new(MqttString::from_str("demo/topic").unwrap()).unwrap();

        client
            .subscribe(topic.as_borrowed().into(), SubscriptionOptions::new())
            .await
            .unwrap();

        let packet_identifier = client
            .publish(
                &PublicationOptions::new(TopicReference::Name(topic)).exactly_once(),
                "Hello World!".into(),
            )
            .await
            .unwrap()
            .unwrap();
    }

    let mut watchdog = wdg::IndependentWatchdog::new(p.IWDG, 1000000);
    watchdog.unleash();
    loop {
        debug!("Status: Alive");
        Timer::after_millis(500).await;
        watchdog.pet();
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    SCB::sys_reset() // <- you could do something other than reset
}

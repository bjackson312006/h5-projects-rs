//! Type aliases for units from the `uom` crate used by this project.

/// Voltage and such
pub type ElectricPotential = uom::si::f32::ElectricPotential;

pub type Temperature = uom::si::f32::ThermodynamicTemperature;

// Custom "microcelcius" unit (not really a real unit but adbms6830b returns temp scaled this way). Can be used with `Temperature`
uom::unit! {
    system: uom::si;
    quantity: uom::si::thermodynamic_temperature;

    @microcelcius: 1.0e-6, 273.15; "uC", "degree (microcelcius)", "degrees (microcelcius)"; 
}
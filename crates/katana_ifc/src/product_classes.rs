//! Every entity that descends from IfcProduct in the IFC schemas the import
//! reads (IFC2X3, IFC4, IFC4X3_ADD2), in the schemas' own spelling, with what
//! the import does with it. An ISO 10303-21 file names its entities in upper
//! case and carries no schema: without this list, a product cannot be told
//! from any other instance, nor reported as "IfcPipeSegment" rather than
//! "IFCPIPESEGMENT".
//!
//! The rows are those of src/katana_ifc/product_classes.inc, which
//! tools/ifc_product_classes.py generates from IfcOpenShell 0.8.5's schemas,
//! each `{"UPPER", "Name", ProductKind::Kind},` written here as
//! `row("UPPER", "Name", ProductKind::Kind),` and nothing else changed. They
//! are sorted by the upper-case name, as the tool sorts them, for the binary
//! search of `product_class`.

/// What the import does with a product of a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProductKind {
    Drawn,
    NotDrawn,
    Spatial,
    Positioning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProductClass {
    pub(crate) upper: &'static str,
    pub(crate) name: &'static str,
    pub(crate) kind: ProductKind,
}

const fn row(upper: &'static str, name: &'static str, kind: ProductKind) -> ProductClass {
    ProductClass { upper, name, kind }
}

pub(crate) const PRODUCT_CLASSES: &[ProductClass] = &[
    row("IFCACTUATOR", "IfcActuator", ProductKind::Drawn),
    row("IFCAIRTERMINAL", "IfcAirTerminal", ProductKind::Drawn),
    row("IFCAIRTERMINALBOX", "IfcAirTerminalBox", ProductKind::Drawn),
    row(
        "IFCAIRTOAIRHEATRECOVERY",
        "IfcAirToAirHeatRecovery",
        ProductKind::Drawn,
    ),
    row("IFCALARM", "IfcAlarm", ProductKind::Drawn),
    row("IFCALIGNMENT", "IfcAlignment", ProductKind::Positioning),
    row(
        "IFCALIGNMENTCANT",
        "IfcAlignmentCant",
        ProductKind::Positioning,
    ),
    row(
        "IFCALIGNMENTHORIZONTAL",
        "IfcAlignmentHorizontal",
        ProductKind::Positioning,
    ),
    row(
        "IFCALIGNMENTSEGMENT",
        "IfcAlignmentSegment",
        ProductKind::Positioning,
    ),
    row(
        "IFCALIGNMENTVERTICAL",
        "IfcAlignmentVertical",
        ProductKind::Positioning,
    ),
    row("IFCANNOTATION", "IfcAnnotation", ProductKind::Drawn),
    row(
        "IFCAUDIOVISUALAPPLIANCE",
        "IfcAudioVisualAppliance",
        ProductKind::Drawn,
    ),
    row("IFCBEAM", "IfcBeam", ProductKind::Drawn),
    row(
        "IFCBEAMSTANDARDCASE",
        "IfcBeamStandardCase",
        ProductKind::Drawn,
    ),
    row("IFCBEARING", "IfcBearing", ProductKind::Drawn),
    row("IFCBOILER", "IfcBoiler", ProductKind::Drawn),
    row("IFCBOREHOLE", "IfcBorehole", ProductKind::Drawn),
    row("IFCBRIDGE", "IfcBridge", ProductKind::Spatial),
    row("IFCBRIDGEPART", "IfcBridgePart", ProductKind::Spatial),
    row("IFCBUILDING", "IfcBuilding", ProductKind::Spatial),
    row(
        "IFCBUILDINGELEMENT",
        "IfcBuildingElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCBUILDINGELEMENTCOMPONENT",
        "IfcBuildingElementComponent",
        ProductKind::Drawn,
    ),
    row(
        "IFCBUILDINGELEMENTPART",
        "IfcBuildingElementPart",
        ProductKind::Drawn,
    ),
    row(
        "IFCBUILDINGELEMENTPROXY",
        "IfcBuildingElementProxy",
        ProductKind::Drawn,
    ),
    row(
        "IFCBUILDINGSTOREY",
        "IfcBuildingStorey",
        ProductKind::Spatial,
    ),
    row("IFCBUILTELEMENT", "IfcBuiltElement", ProductKind::Drawn),
    row("IFCBURNER", "IfcBurner", ProductKind::Drawn),
    row(
        "IFCCABLECARRIERFITTING",
        "IfcCableCarrierFitting",
        ProductKind::Drawn,
    ),
    row(
        "IFCCABLECARRIERSEGMENT",
        "IfcCableCarrierSegment",
        ProductKind::Drawn,
    ),
    row("IFCCABLEFITTING", "IfcCableFitting", ProductKind::Drawn),
    row("IFCCABLESEGMENT", "IfcCableSegment", ProductKind::Drawn),
    row(
        "IFCCAISSONFOUNDATION",
        "IfcCaissonFoundation",
        ProductKind::Drawn,
    ),
    row(
        "IFCCHAMFEREDGEFEATURE",
        "IfcChamferEdgeFeature",
        ProductKind::NotDrawn,
    ),
    row("IFCCHILLER", "IfcChiller", ProductKind::Drawn),
    row("IFCCHIMNEY", "IfcChimney", ProductKind::Drawn),
    row("IFCCIVILELEMENT", "IfcCivilElement", ProductKind::Drawn),
    row("IFCCOIL", "IfcCoil", ProductKind::Drawn),
    row("IFCCOLUMN", "IfcColumn", ProductKind::Drawn),
    row(
        "IFCCOLUMNSTANDARDCASE",
        "IfcColumnStandardCase",
        ProductKind::Drawn,
    ),
    row(
        "IFCCOMMUNICATIONSAPPLIANCE",
        "IfcCommunicationsAppliance",
        ProductKind::Drawn,
    ),
    row("IFCCOMPRESSOR", "IfcCompressor", ProductKind::Drawn),
    row("IFCCONDENSER", "IfcCondenser", ProductKind::Drawn),
    row("IFCCONTROLLER", "IfcController", ProductKind::Drawn),
    row(
        "IFCCONVEYORSEGMENT",
        "IfcConveyorSegment",
        ProductKind::Drawn,
    ),
    row("IFCCOOLEDBEAM", "IfcCooledBeam", ProductKind::Drawn),
    row("IFCCOOLINGTOWER", "IfcCoolingTower", ProductKind::Drawn),
    row("IFCCOURSE", "IfcCourse", ProductKind::Drawn),
    row("IFCCOVERING", "IfcCovering", ProductKind::Drawn),
    row("IFCCURTAINWALL", "IfcCurtainWall", ProductKind::Drawn),
    row("IFCDAMPER", "IfcDamper", ProductKind::Drawn),
    row("IFCDEEPFOUNDATION", "IfcDeepFoundation", ProductKind::Drawn),
    row(
        "IFCDISCRETEACCESSORY",
        "IfcDiscreteAccessory",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONBOARD",
        "IfcDistributionBoard",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONCHAMBERELEMENT",
        "IfcDistributionChamberElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONCONTROLELEMENT",
        "IfcDistributionControlElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONELEMENT",
        "IfcDistributionElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONFLOWELEMENT",
        "IfcDistributionFlowElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCDISTRIBUTIONPORT",
        "IfcDistributionPort",
        ProductKind::NotDrawn,
    ),
    row("IFCDOOR", "IfcDoor", ProductKind::Drawn),
    row(
        "IFCDOORSTANDARDCASE",
        "IfcDoorStandardCase",
        ProductKind::Drawn,
    ),
    row("IFCDUCTFITTING", "IfcDuctFitting", ProductKind::Drawn),
    row("IFCDUCTSEGMENT", "IfcDuctSegment", ProductKind::Drawn),
    row("IFCDUCTSILENCER", "IfcDuctSilencer", ProductKind::Drawn),
    row("IFCEARTHWORKSCUT", "IfcEarthworksCut", ProductKind::Drawn),
    row(
        "IFCEARTHWORKSELEMENT",
        "IfcEarthworksElement",
        ProductKind::Drawn,
    ),
    row("IFCEARTHWORKSFILL", "IfcEarthworksFill", ProductKind::Drawn),
    row("IFCEDGEFEATURE", "IfcEdgeFeature", ProductKind::NotDrawn),
    row(
        "IFCELECTRICALELEMENT",
        "IfcElectricalElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICAPPLIANCE",
        "IfcElectricAppliance",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICDISTRIBUTIONBOARD",
        "IfcElectricDistributionBoard",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICDISTRIBUTIONPOINT",
        "IfcElectricDistributionPoint",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICFLOWSTORAGEDEVICE",
        "IfcElectricFlowStorageDevice",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICFLOWTREATMENTDEVICE",
        "IfcElectricFlowTreatmentDevice",
        ProductKind::Drawn,
    ),
    row(
        "IFCELECTRICGENERATOR",
        "IfcElectricGenerator",
        ProductKind::Drawn,
    ),
    row("IFCELECTRICMOTOR", "IfcElectricMotor", ProductKind::Drawn),
    row(
        "IFCELECTRICTIMECONTROL",
        "IfcElectricTimeControl",
        ProductKind::Drawn,
    ),
    row("IFCELEMENT", "IfcElement", ProductKind::Drawn),
    row(
        "IFCELEMENTASSEMBLY",
        "IfcElementAssembly",
        ProductKind::Drawn,
    ),
    row(
        "IFCELEMENTCOMPONENT",
        "IfcElementComponent",
        ProductKind::Drawn,
    ),
    row(
        "IFCENERGYCONVERSIONDEVICE",
        "IfcEnergyConversionDevice",
        ProductKind::Drawn,
    ),
    row("IFCENGINE", "IfcEngine", ProductKind::Drawn),
    row(
        "IFCEQUIPMENTELEMENT",
        "IfcEquipmentElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCEVAPORATIVECOOLER",
        "IfcEvaporativeCooler",
        ProductKind::Drawn,
    ),
    row("IFCEVAPORATOR", "IfcEvaporator", ProductKind::Drawn),
    row(
        "IFCEXTERNALSPATIALELEMENT",
        "IfcExternalSpatialElement",
        ProductKind::Spatial,
    ),
    row(
        "IFCEXTERNALSPATIALSTRUCTUREELEMENT",
        "IfcExternalSpatialStructureElement",
        ProductKind::Spatial,
    ),
    row("IFCFACILITY", "IfcFacility", ProductKind::Spatial),
    row("IFCFACILITYPART", "IfcFacilityPart", ProductKind::Spatial),
    row(
        "IFCFACILITYPARTCOMMON",
        "IfcFacilityPartCommon",
        ProductKind::Spatial,
    ),
    row("IFCFAN", "IfcFan", ProductKind::Drawn),
    row("IFCFASTENER", "IfcFastener", ProductKind::Drawn),
    row("IFCFEATUREELEMENT", "IfcFeatureElement", ProductKind::Drawn),
    row(
        "IFCFEATUREELEMENTADDITION",
        "IfcFeatureElementAddition",
        ProductKind::Drawn,
    ),
    row(
        "IFCFEATUREELEMENTSUBTRACTION",
        "IfcFeatureElementSubtraction",
        ProductKind::NotDrawn,
    ),
    row("IFCFILTER", "IfcFilter", ProductKind::Drawn),
    row(
        "IFCFIRESUPPRESSIONTERMINAL",
        "IfcFireSuppressionTerminal",
        ProductKind::Drawn,
    ),
    row("IFCFLOWCONTROLLER", "IfcFlowController", ProductKind::Drawn),
    row("IFCFLOWFITTING", "IfcFlowFitting", ProductKind::Drawn),
    row("IFCFLOWINSTRUMENT", "IfcFlowInstrument", ProductKind::Drawn),
    row("IFCFLOWMETER", "IfcFlowMeter", ProductKind::Drawn),
    row(
        "IFCFLOWMOVINGDEVICE",
        "IfcFlowMovingDevice",
        ProductKind::Drawn,
    ),
    row("IFCFLOWSEGMENT", "IfcFlowSegment", ProductKind::Drawn),
    row(
        "IFCFLOWSTORAGEDEVICE",
        "IfcFlowStorageDevice",
        ProductKind::Drawn,
    ),
    row("IFCFLOWTERMINAL", "IfcFlowTerminal", ProductKind::Drawn),
    row(
        "IFCFLOWTREATMENTDEVICE",
        "IfcFlowTreatmentDevice",
        ProductKind::Drawn,
    ),
    row("IFCFOOTING", "IfcFooting", ProductKind::Drawn),
    row(
        "IFCFURNISHINGELEMENT",
        "IfcFurnishingElement",
        ProductKind::Drawn,
    ),
    row("IFCFURNITURE", "IfcFurniture", ProductKind::Drawn),
    row(
        "IFCGEOGRAPHICELEMENT",
        "IfcGeographicElement",
        ProductKind::Drawn,
    ),
    row("IFCGEOMODEL", "IfcGeomodel", ProductKind::Drawn),
    row("IFCGEOSLICE", "IfcGeoslice", ProductKind::Drawn),
    row(
        "IFCGEOTECHNICALASSEMBLY",
        "IfcGeotechnicalAssembly",
        ProductKind::Drawn,
    ),
    row(
        "IFCGEOTECHNICALELEMENT",
        "IfcGeotechnicalElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCGEOTECHNICALSTRATUM",
        "IfcGeotechnicalStratum",
        ProductKind::Drawn,
    ),
    row("IFCGRID", "IfcGrid", ProductKind::Positioning),
    row("IFCHEATEXCHANGER", "IfcHeatExchanger", ProductKind::Drawn),
    row("IFCHUMIDIFIER", "IfcHumidifier", ProductKind::Drawn),
    row(
        "IFCIMPACTPROTECTIONDEVICE",
        "IfcImpactProtectionDevice",
        ProductKind::Drawn,
    ),
    row("IFCINTERCEPTOR", "IfcInterceptor", ProductKind::Drawn),
    row("IFCJUNCTIONBOX", "IfcJunctionBox", ProductKind::Drawn),
    row("IFCKERB", "IfcKerb", ProductKind::Drawn),
    row("IFCLAMP", "IfcLamp", ProductKind::Drawn),
    row("IFCLIGHTFIXTURE", "IfcLightFixture", ProductKind::Drawn),
    row(
        "IFCLINEARELEMENT",
        "IfcLinearElement",
        ProductKind::Positioning,
    ),
    row(
        "IFCLINEARPOSITIONINGELEMENT",
        "IfcLinearPositioningElement",
        ProductKind::Positioning,
    ),
    row("IFCLIQUIDTERMINAL", "IfcLiquidTerminal", ProductKind::Drawn),
    row(
        "IFCMARINEFACILITY",
        "IfcMarineFacility",
        ProductKind::Spatial,
    ),
    row("IFCMARINEPART", "IfcMarinePart", ProductKind::Spatial),
    row(
        "IFCMECHANICALFASTENER",
        "IfcMechanicalFastener",
        ProductKind::Drawn,
    ),
    row("IFCMEDICALDEVICE", "IfcMedicalDevice", ProductKind::Drawn),
    row("IFCMEMBER", "IfcMember", ProductKind::Drawn),
    row(
        "IFCMEMBERSTANDARDCASE",
        "IfcMemberStandardCase",
        ProductKind::Drawn,
    ),
    row(
        "IFCMOBILETELECOMMUNICATIONSAPPLIANCE",
        "IfcMobileTelecommunicationsAppliance",
        ProductKind::Drawn,
    ),
    row("IFCMOORINGDEVICE", "IfcMooringDevice", ProductKind::Drawn),
    row(
        "IFCMOTORCONNECTION",
        "IfcMotorConnection",
        ProductKind::Drawn,
    ),
    row(
        "IFCNAVIGATIONELEMENT",
        "IfcNavigationElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCOPENINGELEMENT",
        "IfcOpeningElement",
        ProductKind::NotDrawn,
    ),
    row(
        "IFCOPENINGSTANDARDCASE",
        "IfcOpeningStandardCase",
        ProductKind::NotDrawn,
    ),
    row("IFCOUTLET", "IfcOutlet", ProductKind::Drawn),
    row("IFCPAVEMENT", "IfcPavement", ProductKind::Drawn),
    row("IFCPILE", "IfcPile", ProductKind::Drawn),
    row("IFCPIPEFITTING", "IfcPipeFitting", ProductKind::Drawn),
    row("IFCPIPESEGMENT", "IfcPipeSegment", ProductKind::Drawn),
    row("IFCPLATE", "IfcPlate", ProductKind::Drawn),
    row(
        "IFCPLATESTANDARDCASE",
        "IfcPlateStandardCase",
        ProductKind::Drawn,
    ),
    row("IFCPORT", "IfcPort", ProductKind::NotDrawn),
    row(
        "IFCPOSITIONINGELEMENT",
        "IfcPositioningElement",
        ProductKind::Positioning,
    ),
    row("IFCPRODUCT", "IfcProduct", ProductKind::Drawn),
    row(
        "IFCPROJECTIONELEMENT",
        "IfcProjectionElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCPROTECTIVEDEVICE",
        "IfcProtectiveDevice",
        ProductKind::Drawn,
    ),
    row(
        "IFCPROTECTIVEDEVICETRIPPINGUNIT",
        "IfcProtectiveDeviceTrippingUnit",
        ProductKind::Drawn,
    ),
    row("IFCPROXY", "IfcProxy", ProductKind::Drawn),
    row("IFCPUMP", "IfcPump", ProductKind::Drawn),
    row("IFCRAIL", "IfcRail", ProductKind::Drawn),
    row("IFCRAILING", "IfcRailing", ProductKind::Drawn),
    row("IFCRAILWAY", "IfcRailway", ProductKind::Spatial),
    row("IFCRAILWAYPART", "IfcRailwayPart", ProductKind::Spatial),
    row("IFCRAMP", "IfcRamp", ProductKind::Drawn),
    row("IFCRAMPFLIGHT", "IfcRampFlight", ProductKind::Drawn),
    row("IFCREFERENT", "IfcReferent", ProductKind::Positioning),
    row("IFCREINFORCEDSOIL", "IfcReinforcedSoil", ProductKind::Drawn),
    row("IFCREINFORCINGBAR", "IfcReinforcingBar", ProductKind::Drawn),
    row(
        "IFCREINFORCINGELEMENT",
        "IfcReinforcingElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCREINFORCINGMESH",
        "IfcReinforcingMesh",
        ProductKind::Drawn,
    ),
    row("IFCROAD", "IfcRoad", ProductKind::Spatial),
    row("IFCROADPART", "IfcRoadPart", ProductKind::Spatial),
    row("IFCROOF", "IfcRoof", ProductKind::Drawn),
    row(
        "IFCROUNDEDEDGEFEATURE",
        "IfcRoundedEdgeFeature",
        ProductKind::NotDrawn,
    ),
    row(
        "IFCSANITARYTERMINAL",
        "IfcSanitaryTerminal",
        ProductKind::Drawn,
    ),
    row("IFCSENSOR", "IfcSensor", ProductKind::Drawn),
    row("IFCSHADINGDEVICE", "IfcShadingDevice", ProductKind::Drawn),
    row("IFCSIGN", "IfcSign", ProductKind::Drawn),
    row("IFCSIGNAL", "IfcSignal", ProductKind::Drawn),
    row("IFCSITE", "IfcSite", ProductKind::Spatial),
    row("IFCSLAB", "IfcSlab", ProductKind::Drawn),
    row(
        "IFCSLABELEMENTEDCASE",
        "IfcSlabElementedCase",
        ProductKind::Drawn,
    ),
    row(
        "IFCSLABSTANDARDCASE",
        "IfcSlabStandardCase",
        ProductKind::Drawn,
    ),
    row("IFCSOLARDEVICE", "IfcSolarDevice", ProductKind::Drawn),
    row("IFCSPACE", "IfcSpace", ProductKind::Spatial),
    row("IFCSPACEHEATER", "IfcSpaceHeater", ProductKind::Drawn),
    row(
        "IFCSPATIALELEMENT",
        "IfcSpatialElement",
        ProductKind::Spatial,
    ),
    row(
        "IFCSPATIALSTRUCTUREELEMENT",
        "IfcSpatialStructureElement",
        ProductKind::Spatial,
    ),
    row("IFCSPATIALZONE", "IfcSpatialZone", ProductKind::Spatial),
    row("IFCSTACKTERMINAL", "IfcStackTerminal", ProductKind::Drawn),
    row("IFCSTAIR", "IfcStair", ProductKind::Drawn),
    row("IFCSTAIRFLIGHT", "IfcStairFlight", ProductKind::Drawn),
    row(
        "IFCSTRUCTURALACTION",
        "IfcStructuralAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALACTIVITY",
        "IfcStructuralActivity",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCONNECTION",
        "IfcStructuralConnection",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCURVEACTION",
        "IfcStructuralCurveAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCURVECONNECTION",
        "IfcStructuralCurveConnection",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCURVEMEMBER",
        "IfcStructuralCurveMember",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCURVEMEMBERVARYING",
        "IfcStructuralCurveMemberVarying",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALCURVEREACTION",
        "IfcStructuralCurveReaction",
        ProductKind::Drawn,
    ),
    row("IFCSTRUCTURALITEM", "IfcStructuralItem", ProductKind::Drawn),
    row(
        "IFCSTRUCTURALLINEARACTION",
        "IfcStructuralLinearAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALLINEARACTIONVARYING",
        "IfcStructuralLinearActionVarying",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALMEMBER",
        "IfcStructuralMember",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALPLANARACTION",
        "IfcStructuralPlanarAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALPLANARACTIONVARYING",
        "IfcStructuralPlanarActionVarying",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALPOINTACTION",
        "IfcStructuralPointAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALPOINTCONNECTION",
        "IfcStructuralPointConnection",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALPOINTREACTION",
        "IfcStructuralPointReaction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALREACTION",
        "IfcStructuralReaction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALSURFACEACTION",
        "IfcStructuralSurfaceAction",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALSURFACECONNECTION",
        "IfcStructuralSurfaceConnection",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALSURFACEMEMBER",
        "IfcStructuralSurfaceMember",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALSURFACEMEMBERVARYING",
        "IfcStructuralSurfaceMemberVarying",
        ProductKind::Drawn,
    ),
    row(
        "IFCSTRUCTURALSURFACEREACTION",
        "IfcStructuralSurfaceReaction",
        ProductKind::Drawn,
    ),
    row("IFCSURFACEFEATURE", "IfcSurfaceFeature", ProductKind::Drawn),
    row(
        "IFCSWITCHINGDEVICE",
        "IfcSwitchingDevice",
        ProductKind::Drawn,
    ),
    row(
        "IFCSYSTEMFURNITUREELEMENT",
        "IfcSystemFurnitureElement",
        ProductKind::Drawn,
    ),
    row("IFCTANK", "IfcTank", ProductKind::Drawn),
    row("IFCTENDON", "IfcTendon", ProductKind::Drawn),
    row("IFCTENDONANCHOR", "IfcTendonAnchor", ProductKind::Drawn),
    row("IFCTENDONCONDUIT", "IfcTendonConduit", ProductKind::Drawn),
    row("IFCTRACKELEMENT", "IfcTrackElement", ProductKind::Drawn),
    row("IFCTRANSFORMER", "IfcTransformer", ProductKind::Drawn),
    row(
        "IFCTRANSPORTATIONDEVICE",
        "IfcTransportationDevice",
        ProductKind::Drawn,
    ),
    row(
        "IFCTRANSPORTELEMENT",
        "IfcTransportElement",
        ProductKind::Drawn,
    ),
    row("IFCTUBEBUNDLE", "IfcTubeBundle", ProductKind::Drawn),
    row(
        "IFCUNITARYCONTROLELEMENT",
        "IfcUnitaryControlElement",
        ProductKind::Drawn,
    ),
    row(
        "IFCUNITARYEQUIPMENT",
        "IfcUnitaryEquipment",
        ProductKind::Drawn,
    ),
    row("IFCVALVE", "IfcValve", ProductKind::Drawn),
    row("IFCVEHICLE", "IfcVehicle", ProductKind::Drawn),
    row(
        "IFCVIBRATIONDAMPER",
        "IfcVibrationDamper",
        ProductKind::Drawn,
    ),
    row(
        "IFCVIBRATIONISOLATOR",
        "IfcVibrationIsolator",
        ProductKind::Drawn,
    ),
    row(
        "IFCVIRTUALELEMENT",
        "IfcVirtualElement",
        ProductKind::NotDrawn,
    ),
    row(
        "IFCVOIDINGFEATURE",
        "IfcVoidingFeature",
        ProductKind::NotDrawn,
    ),
    row("IFCWALL", "IfcWall", ProductKind::Drawn),
    row(
        "IFCWALLELEMENTEDCASE",
        "IfcWallElementedCase",
        ProductKind::Drawn,
    ),
    row(
        "IFCWALLSTANDARDCASE",
        "IfcWallStandardCase",
        ProductKind::Drawn,
    ),
    row("IFCWASTETERMINAL", "IfcWasteTerminal", ProductKind::Drawn),
    row("IFCWINDOW", "IfcWindow", ProductKind::Drawn),
    row(
        "IFCWINDOWSTANDARDCASE",
        "IfcWindowStandardCase",
        ProductKind::Drawn,
    ),
];

/// The class whose upper-case name is `upper`: a lower bound in the sorted
/// table, as the C++'s `std::lower_bound` finds it.
pub(crate) fn product_class(upper: &str) -> Option<&'static ProductClass> {
    let at = PRODUCT_CLASSES.partition_point(|class| class.upper < upper);
    PRODUCT_CLASSES.get(at).filter(|class| class.upper == upper)
}

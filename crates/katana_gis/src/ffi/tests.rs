//! The FFI modules' owners and views, held to what they promise: each frees
//! what it owns once, a borrowed view reads what its owner holds, text and
//! lists come back across the boundary whole, and a callback never lets a
//! panic into C. Headless and without the network: GDAL's in-memory driver
//! and its /vsimem file system, which live in this process alone.

use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use super::cpl::{self, CStringList, VsiFile};
use super::dataset::{self, Dataset, Driver, Feature, FieldDefn, Progress};
use super::error::{CollectingHandler, last_error_message, reset_last_error};
use super::geometry::OgrGeometry;
use super::xml::XmlDocument;
use super::{gdal, minixml, ogr};
use crate::gdal_registry::ensure_registered;

/// A /vsimem folder of this test's own: tests run side by side in one
/// process, and /vsimem is the process's.
fn scratch_folder() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let number = NEXT.fetch_add(1, Ordering::Relaxed);
    let folder = format!("/vsimem/katana-ffi-tests/{}/{number}", std::process::id());
    assert_eq!(cpl::mkdir_recursive(&cpl::c_string(&folder), 0o755), 0);
    folder
}

fn remove_folder(folder: &str) {
    cpl::rmdir_recursive(&cpl::c_string(folder));
}

fn write_file(path: &str, bytes: &[u8]) {
    let mut file = VsiFile::open(&cpl::c_string(path), c"wb").expect("a /vsimem file opens to write");
    assert_eq!(file.write(bytes), bytes.len());
    assert_eq!(file.close(), 0, "a /vsimem file closes cleanly");
}

#[test]
fn a_list_gdal_allocates_comes_back_as_its_strings_and_is_freed() {
    ensure_registered();
    let folder = scratch_folder();
    write_file(&format!("{folder}/one.txt"), b"1");
    write_file(&format!("{folder}/two.txt"), b"22");

    // VSIReadDirRecursive's list is GDAL's (CSLDestroy frees it): what comes
    // back is a copy of each entry, and the list is freed inside the call.
    let mut names = cpl::read_dir_recursive(&cpl::c_string(&folder))
        .expect("a folder with files in it lists them");
    names.sort();
    assert_eq!(names, vec![b"one.txt".to_vec(), b"two.txt".to_vec()]);

    // A folder that is not there is no list at all (NULL), not an empty one.
    let missing = format!("{folder}/not-there");
    assert!(cpl::read_dir_recursive(&cpl::c_string(&missing)).is_none());
    remove_folder(&folder);
}

#[test]
fn a_file_written_and_closed_reads_back_byte_for_byte() {
    ensure_registered();
    let folder = scratch_folder();
    let path = format!("{folder}/bytes.bin");
    let bytes: Vec<u8> = (0..=255).collect();
    write_file(&path, &bytes);

    let status = cpl::stat(&cpl::c_string(&path)).expect("the file written is there");
    assert!(!status.is_dir());
    assert_eq!(status.size(), 256);

    let mut file = VsiFile::open(&cpl::c_string(&path), c"rb").expect("it opens to read");
    let mut read = vec![0u8; 256];
    assert_eq!(file.read(&mut read), 256);
    // Dropped unclosed: Drop closes it, once.
    drop(file);
    assert_eq!(read, bytes);

    // Nothing is asked of GDAL for an empty buffer.
    let mut file = VsiFile::open(&cpl::c_string(&path), c"rb").expect("it opens again");
    assert_eq!(file.read(&mut []), 0);
    assert_eq!(file.close(), 0);
    remove_folder(&folder);
}

#[test]
fn a_dataset_dropped_is_closed_so_what_it_wrote_reads_back_when_reopened() {
    ensure_registered();
    let folder = scratch_folder();
    let path = cpl::c_string(format!("{folder}/grid.tif"));
    let values = [1.0, 2.0, 3.0, 4.5];
    {
        let driver = Driver::by_name("GTiff").expect("this GDAL has GTiff");
        let made = driver.create(&path, 2, 2, 1, gdal::GDT_FLOAT64, &CStringList::new());
        let written = made.expect("GTiff makes a 2 x 2 grid in /vsimem");
        let band = written.band(1).expect("the grid has its one band");
        assert_eq!(band.write_f64(0, 0, 2, 2, &values, 2, 2), cpl::CE_NONE);
        assert_eq!(band.set_no_data_value(-9.0), cpl::CE_NONE);
        // A GeoTIFF's pixels reach the file when it is closed: Drop does it.
    }
    let reopened = Dataset::open(&path, gdal::GDAL_OF_READONLY | gdal::GDAL_OF_RASTER, &CStringList::new())
        .expect("the closed file opens");
    assert_eq!((reopened.raster_x_size(), reopened.raster_y_size()), (2, 2));
    let band = reopened.band(1).expect("one band");
    let mut read = [0.0; 4];
    assert_eq!(band.read_f64(0, 0, 2, 2, &mut read, 2, 2), cpl::CE_NONE);
    assert_eq!(read, values);
    assert_eq!(band.no_data_value(), Some(-9.0));
    // A band GDAL does not have is None, never a pointer to nothing.
    assert!(reopened.band(2).is_none());
    assert_eq!(reopened.close(), cpl::CE_NONE, "a read-only close has nothing to fail at");
    remove_folder(&folder);
}

#[test]
fn a_buffer_too_small_for_the_window_is_refused_without_asking_gdal() {
    ensure_registered();
    let driver = Driver::by_name("MEM").expect("this GDAL has MEM");
    let made = driver.create(c"", 3, 3, 1, gdal::GDT_FLOAT64, &CStringList::new());
    let grid = made.expect("MEM makes a 3 x 3 grid");
    let band = grid.band(1).expect("one band");
    let mut short = [0.0; 8];
    assert_eq!(band.read_f64(0, 0, 3, 3, &mut short, 3, 3), cpl::CE_FAILURE);
    assert_eq!(band.write_f64(0, 0, 3, 3, &short, 3, 3), cpl::CE_FAILURE);
    let mut enough = [0.0; 9];
    assert_eq!(band.read_f64(0, 0, 3, 3, &mut enough, 3, 3), cpl::CE_NONE);
}

#[test]
fn a_failed_open_leaves_gdals_message_as_the_last_error_and_in_the_collector() {
    ensure_registered();
    let missing = cpl::c_string(format!("{}/missing.tif", scratch_folder()));
    reset_last_error();
    assert_eq!(last_error_message(), "", "a reset leaves no message");
    let collector = CollectingHandler::push();
    let opened = Dataset::open(&missing, gdal::GDAL_OF_READONLY | gdal::GDAL_OF_RASTER, &CStringList::new());
    assert!(opened.is_none());
    let message = last_error_message();
    assert!(!message.is_empty(), "GDAL says why it opened nothing");
    let records = collector.records();
    let failure = records
        .iter()
        .find(|record| record.is_failure())
        .expect("the failure reached this thread's collector");
    assert_eq!(failure.message, message);
    drop(records);
    drop(collector);
    reset_last_error();
    assert_eq!(last_error_message(), "");
}

#[test]
fn a_feature_made_for_a_layer_is_written_and_read_back_through_its_views() {
    ensure_registered();
    let driver = Driver::by_name("MEM").expect("this GDAL has MEM");
    let made = driver.create(c"", 0, 0, 0, gdal::GDT_UNKNOWN, &CStringList::new());
    let features = made.expect("MEM makes a vector dataset");
    let layer = features
        .create_layer(c"points", None, ogr::WKB_POINT, &CStringList::new())
        .expect("MEM makes a layer");
    let mut field = FieldDefn::new(c"name", ogr::OFT_STRING);
    field.set_sub_type(ogr::OFST_NONE);
    assert_eq!(layer.create_field(&field, true), ogr::OGRERR_NONE);
    drop(field); // the layer took a copy

    let definition = layer.definition().expect("a layer has a definition");
    assert_eq!(definition.field_count(), 1);
    assert_eq!(definition.field_index(c"name"), 0);
    assert_eq!(definition.field(0).map(|field| field.name()), Some(String::from("name")));
    assert!(definition.field(1).is_none());

    let mut feature = Feature::create(definition).expect("a feature of the definition");
    feature.set_field_string(0, c"first");
    let point = OgrGeometry::create(ogr::WKB_POINT);
    point.set_point_2d(1.5, 2.5);
    assert_eq!(feature.set_geometry_directly(point), ogr::OGRERR_NONE);
    assert_eq!(layer.create_feature(&feature), ogr::OGRERR_NONE);
    drop(feature); // the layer took a copy

    layer.reset_reading();
    let read = layer.next_feature().expect("the feature written");
    assert!(read.is_field_set_and_not_null(0));
    assert_eq!(read.field_as_string(0), "first");
    // A negative index is answered here: OGR would read before its array.
    assert!(!read.is_field_set_and_not_null(-1));
    let geometry = read.geometry().expect("the feature's geometry");
    assert_eq!(geometry.point(0), (1.5, 2.5, 0.0));
    assert!(!geometry.is_3d());
    drop(read);
    assert!(layer.next_feature().is_none(), "one feature, then the end");
    assert_eq!(features.layer_count(), 1);
    assert_eq!(features.layer(0).map(|layer| layer.name()), Some(String::from("points")));
}

#[test]
fn a_progress_callback_that_panics_stops_gdal_rather_than_unwinding_into_it() {
    let going: Progress<'_> = &|_complete: f64| cpl::TRUE;
    let panicking: Progress<'_> = &|_complete: f64| -> c_int { panic!("a progress closure failed") };
    let data = |progress: &Progress<'_>| ptr::from_ref(progress).cast_mut().cast::<c_void>();
    assert_eq!(dataset::progress_trampoline(0.5, ptr::null(), data(&going)), cpl::TRUE);
    assert_eq!(dataset::progress_trampoline(0.5, ptr::null(), data(&panicking)), cpl::FALSE);
    // No data is no callback: GDAL goes on.
    assert_eq!(dataset::progress_trampoline(0.5, ptr::null(), ptr::null_mut()), cpl::TRUE);
}

#[test]
fn a_thread_local_option_set_and_put_back_leaves_the_thread_as_it_was() {
    ensure_registered();
    let key = c"KATANA_FFI_TEST_OPTION";
    assert!(cpl::thread_local_config_option(key).is_none());
    cpl::set_thread_local_config_option(key, Some(c"YES"));
    assert_eq!(cpl::thread_local_config_option(key).as_deref(), Some(c"YES"));
    // Another thread does not see it.
    let elsewhere = std::thread::spawn(move || cpl::thread_local_config_option(key))
        .join()
        .expect("the other thread ends");
    assert!(elsewhere.is_none());
    cpl::set_thread_local_config_option(key, None);
    assert!(cpl::thread_local_config_option(key).is_none());
}

#[test]
fn an_xml_tree_is_read_through_its_views_and_freed_once() {
    ensure_registered();
    let document = XmlDocument::parse(c"<?xml version=\"1.0\"?><list kind=\"a\"><item>one</item><item/></list>")
        .expect("the text is XML");
    let nodes: Vec<String> = document.first().and_siblings().map(|node| node.text()).collect();
    assert_eq!(nodes, vec![String::from("?xml"), String::from("list")]);
    let list = document
        .first()
        .and_siblings()
        .find(|node| node.node_type() == minixml::CXT_ELEMENT && node.text() == "list")
        .expect("the document element");
    let attribute = list.children().next().expect("the attribute comes first");
    assert_eq!(attribute.node_type(), minixml::CXT_ATTRIBUTE);
    assert_eq!(attribute.text(), "kind");
    assert_eq!(attribute.child().map(|value| value.text()), Some(String::from("a")));
    let items: Vec<String> = list
        .children()
        .filter(|node| node.node_type() == minixml::CXT_ELEMENT)
        .map(|item| item.xml_value(None))
        .collect();
    assert_eq!(items, vec![String::from("one"), String::new()]);
    assert_eq!(list.xml_value(Some(c"kind")), "a");
    // Dropped here: the tree is destroyed once, after its last view.
    drop(document);
    // What web_access's parse_xml is known to refuse (the interop tests).
    assert!(XmlDocument::parse(c"not xml at all <").is_none());
}

#[test]
fn a_list_rust_hands_gdal_keeps_its_strings_and_replaces_a_key_in_place() {
    let mut list = CStringList::new();
    assert!(list.is_empty());
    assert!(list.as_ptr().is_null(), "an empty list is NULL, as CPLStringList's is");
    list.add("ONE=1");
    list.set_name_value(b"two", b"2");
    list.set_name_value(b"ONE", b"one");
    let entries: Vec<&[u8]> = list.entries().collect();
    assert_eq!(entries, vec![&b"ONE=one"[..], &b"two=2"[..]]);
    assert_eq!(list.len(), 2);
    assert!(!list.as_ptr().is_null());
}

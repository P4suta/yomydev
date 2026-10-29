#![expect(
    unsafe_code,
    reason = "AVFoundation, Vision, and CoreVideo are reached through Objective-C and C bindings"
)]

use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureOutput,
    AVCaptureSession, AVCaptureSessionPreset640x480, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType, AVMediaTypeVideo,
};
use objc2_core_foundation::CFString;
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidthOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use objc2_vision::{
    VNDetectFaceRectanglesRequest, VNDetectHumanRectanglesRequest, VNImageRequestHandler, VNRequest,
};
use yomydev_core::{CameraFault, Detail, Luma, Sight};

use crate::frames::{self, Frame};

const WARM_UP: Duration = Duration::from_secs(1);
const FRAMES: usize = 3;
const CAPTURE: Duration = Duration::from_millis(4500);
const DEADLINE: Duration = Duration::from_secs(5);
const GRANT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Authorized,
    Denied,
    Restricted,
    Undecided,
    Missing,
}

impl fmt::Display for Access {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Authorized => "authorized",
            Self::Denied => "denied; allow it in System Settings > Privacy & Security > Camera",
            Self::Restricted => "restricted by device management",
            Self::Undecided => "not decided within two minutes",
            Self::Missing => "no camera media type on this Mac",
        })
    }
}

fn video() -> Option<&'static AVMediaType> {
    // SAFETY: AVMediaTypeVideo is an immutable AVFoundation constant.
    unsafe { AVMediaTypeVideo }
}

fn status(video: &AVMediaType) -> AVAuthorizationStatus {
    // SAFETY: the media type is an AVFoundation constant.
    unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) }
}

pub(crate) fn grant() -> Access {
    let Some(video) = video() else {
        return Access::Missing;
    };
    match status(video) {
        AVAuthorizationStatus::Authorized => Access::Authorized,
        AVAuthorizationStatus::Restricted => Access::Restricted,
        AVAuthorizationStatus::NotDetermined => {
            let (decided, decision) = mpsc::sync_channel(1);
            let handler = RcBlock::new(move |granted: Bool| {
                let _queued = decided.try_send(granted.as_bool());
            });
            // SAFETY: the media type is an AVFoundation constant, and the handler is 'static.
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &handler);
            }
            match decision.recv_timeout(GRANT) {
                Ok(true) => Access::Authorized,
                Ok(false) => Access::Denied,
                Err(_) => Access::Undecided,
            }
        },
        _ => Access::Denied,
    }
}

pub(crate) fn look() -> Sight {
    let Some(video) = video() else {
        return Sight::Unavailable(CameraFault::Missing);
    };
    if status(video) != AVAuthorizationStatus::Authorized {
        return Sight::Unavailable(CameraFault::NotAuthorized);
    }
    let (finished, outcome) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("yomydev-camera".to_owned())
        .spawn(move || {
            let _delivered = finished.send(capture());
        });
    if worker.is_err() {
        return Sight::Unavailable(CameraFault::Failed);
    }
    match outcome.recv_timeout(DEADLINE) {
        Ok(sight) => sight,
        Err(_) => Sight::Unavailable(CameraFault::TimedOut),
    }
}

fn capture() -> Sight {
    let Some(video) = video() else {
        return Sight::Unavailable(CameraFault::Missing);
    };
    // SAFETY: AVCaptureDeviceTypeBuiltInWideAngleCamera is an immutable AVFoundation constant.
    let built_in = unsafe { AVCaptureDeviceTypeBuiltInWideAngleCamera };
    // SAFETY: both arguments are AVFoundation constants.
    let device = unsafe {
        AVCaptureDevice::defaultDeviceWithDeviceType_mediaType_position(
            built_in,
            Some(video),
            AVCaptureDevicePosition::Unspecified,
        )
    };
    let Some(device) = device else {
        return Sight::Unavailable(CameraFault::Missing);
    };
    // SAFETY: the device came from AVFoundation; a busy or vanished camera returns an error.
    let Ok(input) = (unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }) else {
        return Sight::Unavailable(CameraFault::Failed);
    };
    // SAFETY: a new session has no preconditions.
    let session = unsafe { AVCaptureSession::new() };
    // SAFETY: AVCaptureSessionPreset640x480 is an immutable AVFoundation constant.
    let preset = unsafe { AVCaptureSessionPreset640x480 };
    // SAFETY: the session is not running, so its configuration may change.
    if unsafe { session.canSetSessionPreset(preset) } {
        // SAFETY: the session reported that it accepts this preset.
        unsafe { session.setSessionPreset(preset) };
    }
    // SAFETY: the session is not running, so its configuration may change.
    if !unsafe { session.canAddInput(&input) } {
        return Sight::Unavailable(CameraFault::Failed);
    }
    // SAFETY: the session reported that it accepts this input.
    unsafe { session.addInput(&input) };
    let (sender, frames) = mpsc::sync_channel(FRAMES);
    let sink = FrameSink::new(sender);
    let queue = DispatchQueue::new("io.github.p4suta.yomydev.camera", None);
    let output = video_output(&sink, &queue);
    // SAFETY: the session is not running, so its configuration may change.
    if !unsafe { session.canAddOutput(&output) } {
        return Sight::Unavailable(CameraFault::Failed);
    }
    // SAFETY: the session reported that it accepts this output.
    unsafe { session.addOutput(&output) };
    let running = Running::start(&session);
    let collected = collect(&frames);
    drop(running);
    frames::summarize(&collected)
}

fn video_output(sink: &FrameSink, queue: &DispatchQueue) -> Retained<AVCaptureVideoDataOutput> {
    // SAFETY: a new output has no preconditions.
    let output = unsafe { AVCaptureVideoDataOutput::new() };
    // SAFETY: the output is not attached to a session yet.
    unsafe { output.setAlwaysDiscardsLateVideoFrames(true) };
    let settings = settings();
    // SAFETY: the settings name a pixel format AVFoundation supports.
    unsafe { output.setVideoSettings(Some(&settings)) };
    // SAFETY: the sink stays alive for the whole capture, and the queue is serial.
    unsafe {
        output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(sink)), Some(queue));
    }
    output
}

fn pixel_format_key() -> &'static NSString {
    // SAFETY: kCVPixelBufferPixelFormatTypeKey is an immutable CoreVideo constant.
    let key: &'static CFString = unsafe { kCVPixelBufferPixelFormatTypeKey };
    // SAFETY: CFString and NSString are toll-free bridged.
    unsafe { &*std::ptr::from_ref(key).cast::<NSString>() }
}

fn settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
    let format: &AnyObject = &format;
    NSDictionary::from_slices(&[pixel_format_key()], &[format])
}

fn collect(frames: &Receiver<Option<Frame>>) -> Vec<Option<Frame>> {
    let started = Instant::now();
    let mut collected = Vec::with_capacity(FRAMES);
    while collected.len() < FRAMES {
        let remaining = CAPTURE.saturating_sub(started.elapsed());
        match frames.recv_timeout(remaining) {
            Ok(frame) => collected.push(frame),
            Err(_) => break,
        }
    }
    collected
}

struct Running<'session>(&'session AVCaptureSession);

impl<'session> Running<'session> {
    fn start(session: &'session AVCaptureSession) -> Self {
        // SAFETY: the session is fully configured and runs off the main thread.
        unsafe { session.startRunning() };
        Self(session)
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        // SAFETY: this guard started the session.
        unsafe { self.0.stopRunning() };
    }
}

struct Sink {
    started: Option<Instant>,
    sent: usize,
    frames: SyncSender<Option<Frame>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and FrameSink does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "YomydevFrameSink"]
    #[ivars = Mutex<Sink>]
    struct FrameSink;

    unsafe impl NSObjectProtocol for FrameSink {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for FrameSink {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output_sample_buffer(
            &self,
            _output: &AVCaptureOutput,
            sample: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            self.receive(sample);
        }
    }
);

impl FrameSink {
    fn new(frames: SyncSender<Option<Frame>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Mutex::new(Sink {
            started: None,
            sent: 0,
            frames,
        }));
        // SAFETY: NSObject's init has no preconditions.
        unsafe { msg_send![super(this), init] }
    }

    fn receive(&self, sample: &CMSampleBuffer) {
        let mut sink = self.ivars().lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let started = *sink.started.get_or_insert(now);
        if now.saturating_duration_since(started) < WARM_UP || sink.sent >= FRAMES {
            return;
        }
        sink.sent = sink.sent.saturating_add(1);
        let _queued = sink.frames.try_send(analyse(sample));
    }
}

fn analyse(sample: &CMSampleBuffer) -> Option<Frame> {
    let (luma, detail) = measure(sample)?;
    Some(Frame {
        person: person(sample)?,
        luma,
        detail,
    })
}

fn person(sample: &CMSampleBuffer) -> Option<bool> {
    let options = NSDictionary::new();
    // SAFETY: the sample buffer is valid for this callback, and the options are empty.
    let handler = unsafe {
        VNImageRequestHandler::initWithCMSampleBuffer_options(
            VNImageRequestHandler::alloc(),
            sample,
            &options,
        )
    };
    // SAFETY: a new request has no preconditions.
    let faces = unsafe { VNDetectFaceRectanglesRequest::new() };
    // SAFETY: a new request has no preconditions.
    let bodies = unsafe { VNDetectHumanRectanglesRequest::new() };
    // SAFETY: the request has not run yet.
    unsafe { bodies.setUpperBodyOnly(true) };
    let requests: [&VNRequest; 2] = [&faces, &bodies];
    if handler
        .performRequests_error(&NSArray::from_slice(&requests))
        .is_err()
    {
        return None;
    }
    // SAFETY: the requests finished above.
    let face = unsafe { faces.results() }.is_some_and(|found| !found.is_empty());
    // SAFETY: the requests finished above.
    let body = unsafe { bodies.results() }.is_some_and(|found| !found.is_empty());
    Some(face || body)
}

fn measure(sample: &CMSampleBuffer) -> Option<(Luma, Detail)> {
    // SAFETY: the sample buffer is valid for this callback.
    let pixels = unsafe { sample.image_buffer() }?;
    if CVPixelBufferGetPixelFormatType(&pixels) != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
        return None;
    }
    let locked = Locked::read(&pixels)?;
    let base = CVPixelBufferGetBaseAddressOfPlane(&pixels, 0);
    let width = CVPixelBufferGetWidthOfPlane(&pixels, 0);
    let height = CVPixelBufferGetHeightOfPlane(&pixels, 0);
    let stride = CVPixelBufferGetBytesPerRowOfPlane(&pixels, 0);
    if base.is_null() {
        return None;
    }
    let length = stride.checked_mul(height)?;
    // SAFETY: the buffer stays locked until `locked` drops, and plane 0 spans stride × height bytes.
    let plane = unsafe { std::slice::from_raw_parts(base.cast::<u8>().cast_const(), length) };
    let measured = frames::measure(plane, width, height, stride);
    drop(locked);
    measured
}

struct Locked<'buffer>(&'buffer CVPixelBuffer);

impl<'buffer> Locked<'buffer> {
    fn read(pixels: &'buffer CVPixelBuffer) -> Option<Self> {
        // SAFETY: the pixel buffer is valid, and read-only locking has no other precondition.
        let status =
            unsafe { CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) };
        (status == kCVReturnSuccess).then_some(Self(pixels))
    }
}

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        // SAFETY: this guard locked the buffer with the same flags.
        let _status =
            unsafe { CVPixelBufferUnlockBaseAddress(self.0, CVPixelBufferLockFlags::ReadOnly) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorization_status_reads_without_prompting() {
        let video = video().unwrap();
        let known = [
            AVAuthorizationStatus::NotDetermined,
            AVAuthorizationStatus::Restricted,
            AVAuthorizationStatus::Denied,
            AVAuthorizationStatus::Authorized,
        ];
        assert!(known.contains(&status(video)));
    }

    #[test]
    fn the_settings_ask_for_video_range_luma_planes() {
        let settings = settings();
        assert_eq!(settings.count(), 1);
        let format = settings
            .objectForKey(pixel_format_key())
            .unwrap()
            .downcast::<NSNumber>()
            .unwrap();
        assert_eq!(
            format.as_u32(),
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        );
    }

    #[test]
    fn a_new_frame_sink_has_sent_nothing_and_owns_the_channel() {
        let (sender, frames) = mpsc::sync_channel(1);
        let sink = FrameSink::new(sender);
        let state = sink.ivars().lock().unwrap();
        assert_eq!(state.sent, 0);
        assert!(state.started.is_none());
        state.frames.try_send(None).unwrap();
        drop(state);
        assert_eq!(frames.try_recv(), Ok(None));
        let _delegate: &ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate> =
            ProtocolObject::from_ref(&*sink);
    }
}

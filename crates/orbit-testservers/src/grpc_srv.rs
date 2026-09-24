//! gRPC echo server (with Server Reflection), used for gRPC protocol acceptance runs.

use tonic::{transport::Server, Request, Response, Status};
use tonic_reflection::server::Builder as ReflectionBuilder;

pub mod accept {
    tonic::include_proto!("orbit.accept");
}

use accept::echo_server::{Echo, EchoServer};
use accept::{EchoRequest, EchoResponse};
use futures_util::StreamExt;

#[derive(Default)]
pub struct EchoSvc;

#[tonic::async_trait]
impl Echo for EchoSvc {
    async fn unary(&self, req: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        let msg = req.get_ref();
        Ok(Response::new(EchoResponse {
            message: format!("echo:{}", msg.message),
            seq: 1,
        }))
    }

    type ServerStreamStream = tokio_stream::wrappers::ReceiverStream<Result<EchoResponse, Status>>;

    async fn server_stream(
        &self,
        req: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        let msg = req.get_ref();
        let count = msg.count.max(1) as usize;
        let (tx, rx) = tokio::sync::mpsc::channel(count);
        for i in 0..count {
            tx.send(Ok(EchoResponse {
                message: format!("echo:{}", msg.message),
                seq: i as i32,
            }))
            .await
            .map_err(|_| Status::internal("send failed"))?;
        }
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn client_stream(
        &self,
        req: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<EchoResponse>, Status> {
        let mut stream = req.into_inner();
        let mut count = 0i32;
        while let Some(msg) = stream.next().await {
            let msg = msg.map_err(|e| Status::internal(e.to_string()))?;
            count += 1;
            let _ = msg;
        }
        Ok(Response::new(EchoResponse {
            message: format!("received:{}", count),
            seq: count,
        }))
    }

    type BidiStream = tokio_stream::wrappers::ReceiverStream<Result<EchoResponse, Status>>;

    async fn bidi(
        &self,
        req: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidiStream>, Status> {
        let mut stream = req.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            let mut seq = 0i32;
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(m) => {
                        let _ = tx
                            .send(Ok(EchoResponse {
                                message: format!("echo:{}", m.message),
                                seq,
                            }))
                            .await;
                        seq += 1;
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }
}

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Serve both the v1 and v1alpha reflection protocols (the Orbit client uses v1alpha)
    let reflection_v1 = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(tonic::include_file_descriptor_set!(
            "accept_descriptor"
        ))
        .build_v1()?;
    let reflection_v1alpha = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(tonic::include_file_descriptor_set!(
            "accept_descriptor"
        ))
        .build_v1alpha()?;

    Server::builder()
        .add_service(EchoServer::new(EchoSvc))
        .add_service(reflection_v1)
        .add_service(reflection_v1alpha)
        .serve(format!("127.0.0.1:{port}").parse()?)
        .await?;
    Ok(())
}

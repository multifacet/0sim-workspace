//! Utilities for setting up and running hadoop and spark.

use std::path::Path;

use spurs::{cmd, Execute, SshShell};

const APACHE_HADOOP_MIRROR: &str = "http://apache-mirror.8birdsvideo.com/";

const HADOOP_TARBALL_URL_TEMPLATE: &str = "hadoop/common/hadoop-VERSION/hadoop-VERSION.tar.gz";
const SPARK_TARBALL_URL_TEMPLATE: &str = "spark/spark-VERSION/spark-VERSION-bin-without-hadoop.tgz";

/// Download and untar the hadoop tarball for the given version as `path/hadoop/`, deleting
/// anything that was previously there.
pub fn download_hadoop_tarball<P>(
    ushell: &SshShell,
    version: &str,
    path: &P,
) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
{
    let url =
        APACHE_HADOOP_MIRROR.to_owned() + &HADOOP_TARBALL_URL_TEMPLATE.replace("VERSION", version);

    with_shell! { ushell =>
        cmd!("wget -O /tmp/hadoop.tgz {}", url),
        cmd!("tar xvzf /tmp/hadoop.tgz"),
        cmd!("rm -rf {}/hadoop", path.as_ref().display()),
        cmd!("mv hadoop-{} {}/hadoop", version, path.as_ref().display()),
    }

    Ok(())
}

/// Download and untar the spark tarball for the given version as `$HOME/hadoop/`.
pub fn download_spark_tarball<P>(
    ushell: &SshShell,
    version: &str,
    path: &P,
) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
{
    let url =
        APACHE_HADOOP_MIRROR.to_owned() + &SPARK_TARBALL_URL_TEMPLATE.replace("VERSION", version);

    with_shell! { ushell =>
        cmd!("wget -O /tmp/spark.tgz {}", url),
        cmd!("tar xvzf /tmp/spark.tgz"),
        cmd!("rm -rf {}/spark", path.as_ref().display()),
        cmd!("mv spark-{}-bin-without-hadoop {}/spark", version, path.as_ref().display()),
    }

    Ok(())
}
